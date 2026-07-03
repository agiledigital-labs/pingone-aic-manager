//! API-key-authenticated transport for `/monitoring/logs`.
//!
//! These requests deliberately bypass the bearer-authenticated agent
//! `ApiCall` path. Endpoint and paging details are verified in
//! `docs/api/08-logs.md`.

use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use reqwest::header::RETRY_AFTER;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::logs::LogKeyPair;
use crate::{Error, Result};

pub const PAGE_SIZE: usize = 1000;
const REQUEST_INTERVAL: Duration = Duration::from_millis(1050);
const MAX_429_RETRIES: u32 = 6;
const LOGS_PATH: &str = "/monitoring/logs";

type QueryParams = Vec<(String, String)>;

// Log requests are globally serialized at slightly over one second apart.
// This is conservative across tenants, but guarantees this process stays
// below the documented 60 requests/minute/environment limit.
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::const_new(None);

#[derive(Debug, Clone)]
pub struct LogPage {
    pub result: Vec<Value>,
    pub paged_results_cookie: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SourcesResponse {
    result: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PageResponse {
    result: Vec<Value>,
    #[serde(rename = "pagedResultsCookie")]
    paged_results_cookie: Option<String>,
}

pub async fn sources(client: &Client, base_url: &str, key: &LogKeyPair) -> Result<Vec<String>> {
    let response: SourcesResponse =
        get_json(client, &url(base_url, "/monitoring/logs/sources"), key, &[]).await?;
    Ok(response.result)
}

pub async fn fetch_page(
    client: &Client,
    base_url: &str,
    key: &LogKeyPair,
    query_params: &[(String, String)],
) -> Result<LogPage> {
    let response: PageResponse =
        get_json(client, &url(base_url, LOGS_PATH), key, query_params).await?;
    Ok(LogPage {
        result: response.result,
        paged_results_cookie: response
            .paged_results_cookie
            .filter(|cookie| !cookie.is_empty()),
    })
}

pub async fn fetch_all(
    client: &Client,
    base_url: &str,
    key: &LogKeyPair,
    base_params: &[(String, String)],
) -> Result<Vec<Value>> {
    collect_pages(base_params, |params| async move {
        fetch_page(client, base_url, key, &params).await
    })
    .await
}

pub async fn fetch_transaction(
    client: &Client,
    base_url: &str,
    key: &LogKeyPair,
    transaction_id: &str,
    sources: &[String],
) -> Result<Vec<Value>> {
    let params = vec![
        ("source".to_string(), source_param(sources)?),
        ("transactionId".to_string(), transaction_id.to_string()),
    ];
    fetch_all(client, base_url, key, &params).await
}

pub async fn fetch_range(
    client: &Client,
    base_url: &str,
    key: &LogKeyPair,
    begin: DateTime<Utc>,
    end: DateTime<Utc>,
    sources: &[String],
    query: Option<&str>,
) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    {
        let mut on_page = |page: Vec<Value>| -> Result<()> {
            result.extend(page);
            Ok(())
        };
        fetch_range_streamed(
            client,
            base_url,
            key,
            begin,
            end,
            sources,
            query,
            &mut on_page,
        )
        .await?;
    }

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub async fn fetch_range_streamed(
    client: &Client,
    base_url: &str,
    key: &LogKeyPair,
    begin: DateTime<Utc>,
    end: DateTime<Utc>,
    sources: &[String],
    query: Option<&str>,
    on_page: &mut (dyn FnMut(Vec<Value>) -> Result<()> + Send),
) -> Result<()> {
    fetch_range_streamed_with_fetcher(
        begin,
        end,
        sources,
        query,
        |params| async move { fetch_page(client, base_url, key, &params).await },
        on_page,
    )
    .await
}

pub(crate) fn source_param(sources: &[String]) -> Result<String> {
    if sources.is_empty() {
        return Err(Error::Config("at least one log source is required".into()));
    }
    Ok(sources.join(","))
}

pub(crate) fn split_time_windows(
    begin: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<(DateTime<Utc>, DateTime<Utc>)>> {
    if end <= begin {
        return Err(Error::Config(
            "log range end must be after begin".to_string(),
        ));
    }

    let max_window = ChronoDuration::hours(24);
    let mut windows = Vec::new();
    let mut cursor = begin;
    while cursor < end {
        let window_end = (cursor + max_window).min(end);
        windows.push((cursor, window_end));
        cursor = window_end;
    }
    Ok(windows)
}

async fn collect_pages<F, Fut>(base_params: &[(String, String)], mut fetch: F) -> Result<Vec<Value>>
where
    F: FnMut(QueryParams) -> Fut,
    Fut: Future<Output = Result<LogPage>>,
{
    let mut result = Vec::new();
    {
        let mut on_page = |page: Vec<Value>| -> Result<()> {
            result.extend(page);
            Ok(())
        };
        stream_pages(base_params, &mut fetch, &mut on_page).await?;
    }

    Ok(result)
}

async fn fetch_range_streamed_with_fetcher<F, Fut, OnPage>(
    begin: DateTime<Utc>,
    end: DateTime<Utc>,
    sources: &[String],
    query: Option<&str>,
    mut fetch: F,
    on_page: &mut OnPage,
) -> Result<()>
where
    F: FnMut(QueryParams) -> Fut,
    Fut: Future<Output = Result<LogPage>>,
    OnPage: FnMut(Vec<Value>) -> Result<()> + ?Sized,
{
    let source = source_param(sources)?;

    for (window_begin, window_end) in split_time_windows(begin, end)? {
        let mut params = vec![
            ("source".to_string(), source.clone()),
            ("beginTime".to_string(), wire_time(window_begin)),
            ("endTime".to_string(), wire_time(window_end)),
        ];
        if let Some(query) = query {
            params.push(("_queryFilter".to_string(), query.to_string()));
        }
        stream_pages(&params, &mut fetch, on_page).await?;
    }

    Ok(())
}

async fn stream_pages<F, Fut, OnPage>(
    base_params: &[(String, String)],
    mut fetch: F,
    on_page: &mut OnPage,
) -> Result<()>
where
    F: FnMut(QueryParams) -> Fut,
    Fut: Future<Output = Result<LogPage>>,
    OnPage: FnMut(Vec<Value>) -> Result<()> + ?Sized,
{
    let mut cookie = None;
    let mut seen_cookies = HashSet::new();

    loop {
        let mut params = base_params
            .iter()
            .filter(|(name, _)| name != "_pagedResultsCookie")
            .cloned()
            .collect::<QueryParams>();
        if !params.iter().any(|(name, _)| name == "_pageSize") {
            params.push(("_pageSize".to_string(), PAGE_SIZE.to_string()));
        }
        if let Some(cookie) = cookie.take() {
            params.push(("_pagedResultsCookie".to_string(), cookie));
        }

        let page = fetch(params).await?;
        on_page(page.result)?;
        match page.paged_results_cookie {
            Some(next) if !next.is_empty() => {
                if !seen_cookies.insert(next.clone()) {
                    return Err(Error::Api {
                        status: 0,
                        body: format!("log API repeated paging cookie {next:?}"),
                    });
                }
                cookie = Some(next);
            }
            _ => break,
        }
    }

    Ok(())
}

async fn get_json<T: DeserializeOwned>(
    client: &Client,
    url: &str,
    key: &LogKeyPair,
    query_params: &[(String, String)],
) -> Result<T> {
    let mut retry = 0;
    loop {
        throttle().await;
        let request_url = with_query(url, query_params)?;
        let response = client
            .get(request_url)
            .header("x-api-key", &key.api_key_id)
            .header("x-api-secret", &key.api_key_secret)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = response.status();

        if status == StatusCode::TOO_MANY_REQUESTS && retry < MAX_429_RETRIES {
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or_default();
            let exponential = Duration::from_secs(1_u64 << retry.min(6));
            tokio::time::sleep(retry_after.max(exponential)).await;
            retry += 1;
            continue;
        }

        let bytes = response.bytes().await?;
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        return Ok(serde_json::from_slice(&bytes)?);
    }
}

async fn throttle() {
    let mut last_request = LAST_REQUEST.lock().await;
    if let Some(last) = *last_request {
        let elapsed = last.elapsed();
        if elapsed < REQUEST_INTERVAL {
            tokio::time::sleep(REQUEST_INTERVAL - elapsed).await;
        }
    }
    *last_request = Some(Instant::now());
}

fn wire_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn url(base_url: &str, path: &str) -> String {
    format!("{}{path}", base_url.trim_end_matches('/'))
}

fn with_query(url: &str, query_params: &[(String, String)]) -> Result<url::Url> {
    let mut url = url::Url::parse(url)?;
    url.query_pairs_mut().extend_pairs(query_params);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    fn param_value(params: &[(String, String)], name: &str) -> Option<String> {
        params
            .iter()
            .find(|(param_name, _)| param_name == name)
            .map(|(_, value)| value.clone())
    }

    #[test]
    fn fifty_hour_range_splits_into_three_windows() {
        let begin = Utc.with_ymd_and_hms(2026, 6, 20, 1, 2, 3).unwrap();
        let end = begin + ChronoDuration::hours(50);

        let windows = split_time_windows(begin, end).unwrap();

        assert_eq!(
            windows,
            vec![
                (begin, begin + ChronoDuration::hours(24)),
                (
                    begin + ChronoDuration::hours(24),
                    begin + ChronoDuration::hours(48),
                ),
                (begin + ChronoDuration::hours(48), end),
            ]
        );
    }

    #[test]
    fn sources_are_joined_as_one_comma_separated_param() {
        let sources = vec!["am-everything".to_string(), "idm-everything".to_string()];
        assert_eq!(
            source_param(&sources).unwrap(),
            "am-everything,idm-everything"
        );
    }

    #[tokio::test]
    async fn page_accumulator_passes_cookies_until_empty() {
        let pages = vec![
            LogPage {
                result: vec![json!({"page": 1})],
                paged_results_cookie: Some("next-1".into()),
            },
            LogPage {
                result: vec![json!({"page": 2})],
                paged_results_cookie: Some("next-2".into()),
            },
            LogPage {
                result: vec![json!({"page": 3})],
                paged_results_cookie: None,
            },
        ];
        let mut pages = pages.into_iter();
        let mut observed = Vec::new();

        let result = collect_pages(&[("source".into(), "am-everything".into())], |params| {
            observed.push(params);
            let page = pages.next().expect("expected another page");
            async move { Ok(page) }
        })
        .await
        .unwrap();

        assert_eq!(
            result,
            vec![json!({"page": 1}), json!({"page": 2}), json!({"page": 3})]
        );
        assert_eq!(
            observed
                .iter()
                .map(|params| {
                    params
                        .iter()
                        .find(|(name, _)| name == "_pagedResultsCookie")
                        .map(|(_, value)| value.as_str())
                })
                .collect::<Vec<_>>(),
            vec![None, Some("next-1"), Some("next-2")]
        );
        assert!(observed.iter().all(|params| {
            params
                .iter()
                .any(|(name, value)| name == "_pageSize" && value == "1000")
        }));
    }

    #[tokio::test]
    async fn range_streamer_invokes_callback_once_per_page() {
        let begin = Utc.with_ymd_and_hms(2026, 6, 20, 1, 2, 3).unwrap();
        let end = begin + ChronoDuration::hours(25);
        let sources = vec!["idm-everything".to_string()];
        let pages = vec![
            LogPage {
                result: vec![json!({"page": 1})],
                paged_results_cookie: Some("next-1".into()),
            },
            LogPage {
                result: vec![json!({"page": 2})],
                paged_results_cookie: None,
            },
            LogPage {
                result: vec![json!({"page": 3})],
                paged_results_cookie: None,
            },
        ];
        let mut pages = pages.into_iter();
        let mut observed = Vec::new();
        let mut callback_pages = Vec::new();

        {
            let mut on_page = |page: Vec<Value>| -> Result<()> {
                callback_pages.push(page);
                Ok(())
            };
            fetch_range_streamed_with_fetcher(
                begin,
                end,
                &sources,
                Some("payload/level eq \"INFO\""),
                |params| {
                    observed.push(params);
                    let page = pages.next().expect("expected another page");
                    async move { Ok(page) }
                },
                &mut on_page,
            )
            .await
            .unwrap();
        }

        assert!(pages.next().is_none());
        assert_eq!(
            callback_pages,
            vec![
                vec![json!({"page": 1})],
                vec![json!({"page": 2})],
                vec![json!({"page": 3})],
            ]
        );
        assert_eq!(
            observed
                .iter()
                .map(|params| param_value(params, "_pagedResultsCookie"))
                .collect::<Vec<_>>(),
            vec![None, Some("next-1".to_string()), None]
        );
        assert_eq!(
            param_value(&observed[0], "beginTime"),
            Some(wire_time(begin))
        );
        assert_eq!(
            param_value(&observed[0], "endTime"),
            Some(wire_time(begin + ChronoDuration::hours(24)))
        );
        assert_eq!(
            param_value(&observed[2], "beginTime"),
            Some(wire_time(begin + ChronoDuration::hours(24)))
        );
        assert_eq!(param_value(&observed[2], "endTime"), Some(wire_time(end)));
        assert!(observed.iter().all(|params| {
            param_value(params, "source") == Some("idm-everything".to_string())
                && param_value(params, "_queryFilter")
                    == Some("payload/level eq \"INFO\"".to_string())
                && param_value(params, "_pageSize") == Some("1000".to_string())
        }));
    }
}
