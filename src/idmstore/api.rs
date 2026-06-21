//! HTTP wrappers for IDM managed-object records.
//!
//! Managed-object record endpoints are tenant-global IDM paths. See
//! `docs/api/10-managed-objects.md`.

use std::collections::HashSet;
use std::future::Future;

use serde_json::Value;
use url::form_urlencoded::Serializer;

use crate::{Error, Result};

pub const PAGE_SIZE: usize = 1000;
pub const MAX_CONCURRENCY: usize = 8;
pub const USER_RECORD_FIELDS: &str = "*,_meta/_id,_meta/lastChanged";

#[derive(Debug, Clone)]
pub struct Page {
    pub results: Vec<Value>,
    pub cookie: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaChange {
    pub meta_id: String,
    pub changed: String,
}

pub async fn probe_incremental_supported(tenant: &str, object: &str) -> Result<bool> {
    let sidecar = format!("{object}meta");
    let path = list_path(&sidecar, &[("_queryFilter", "true"), ("_pageSize", "1")]);
    match crate::aic::api::get(tenant, &path).await {
        Ok(_) => Ok(true),
        Err(Error::Api { status: 404, .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

pub async fn list_records_cursor_page(
    tenant: &str,
    object: &str,
    cookie: Option<&str>,
    fields: Option<&str>,
) -> Result<Page> {
    let page_size = PAGE_SIZE.to_string();
    let mut params = vec![("_queryFilter", "true"), ("_pageSize", page_size.as_str())];
    if let Some(cookie) = cookie {
        params.push(("_pagedResultsCookie", cookie));
    }
    if let Some(fields) = fields {
        params.push(("_fields", fields));
    }
    let path = list_path(object, &params);
    let body = crate::aic::api::get(tenant, &path).await?;
    parse_page(&body)
}

pub async fn list_record_ids(tenant: &str, object: &str) -> Result<Vec<String>> {
    let tenant = tenant.to_string();
    let object = object.to_string();
    let records =
        collect_cursor_pages(|cookie| {
            let tenant = tenant.clone();
            let object = object.clone();
            async move {
                list_records_cursor_page(&tenant, &object, cookie.as_deref(), Some("_id")).await
            }
        })
        .await?;

    records.iter().map(required_id).collect::<Result<Vec<_>>>()
}

pub async fn list_changed_meta(
    tenant: &str,
    object: &str,
    watermark: &str,
) -> Result<Vec<MetaChange>> {
    let tenant = tenant.to_string();
    let sidecar = format!("{object}meta");
    let watermark = watermark.to_string();
    let records = collect_cursor_pages(|cookie| {
        let tenant = tenant.clone();
        let sidecar = sidecar.clone();
        let watermark = watermark.clone();
        async move { list_meta_cursor_page(&tenant, &sidecar, &watermark, cookie.as_deref()).await }
    })
    .await?;

    records
        .iter()
        .map(|record| {
            Ok(MetaChange {
                meta_id: required_id(record)?,
                changed: record
                    .pointer("/lastChanged/date")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| Error::Api {
                        status: 0,
                        body: format!("changed meta record has no lastChanged.date: {record}"),
                    })?,
            })
        })
        .collect()
}

pub async fn get_record(
    tenant: &str,
    object: &str,
    id: &str,
    include_meta: bool,
) -> Result<Option<Value>> {
    let mut path = format!(
        "/openidm/managed/{}/{}",
        path_segment(object),
        path_segment(id)
    );
    if include_meta {
        let mut query = Serializer::new(String::new());
        query.append_pair("_fields", USER_RECORD_FIELDS);
        path.push('?');
        path.push_str(&query.finish());
    }
    match crate::aic::api::get(tenant, &path).await {
        Ok(value) => Ok(Some(value)),
        Err(Error::Api { status: 404, .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

pub async fn collect_cursor_pages<F, Fut>(mut source: F) -> Result<Vec<Value>>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Page>>,
{
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut cookie = None;

    loop {
        let page = source(cookie.take()).await?;
        push_dedup(&mut out, &mut seen, page.results);
        match page.cookie {
            Some(next) if !next.is_empty() => cookie = Some(next),
            _ => break,
        }
    }

    Ok(out)
}

fn list_meta_cursor_page(
    tenant: &str,
    sidecar: &str,
    watermark: &str,
    cookie: Option<&str>,
) -> impl Future<Output = Result<Page>> {
    let filter = format!("lastChanged/date ge \"{watermark}\"");
    let page_size = PAGE_SIZE.to_string();
    let mut params = vec![
        ("_queryFilter", filter.as_str()),
        ("_sortKeys", "-lastChanged/date"),
        ("_fields", "_id,lastChanged"),
        ("_pageSize", page_size.as_str()),
    ];
    if let Some(cookie) = cookie {
        params.push(("_pagedResultsCookie", cookie));
    }
    let path = list_path(sidecar, &params);
    async move {
        let body = crate::aic::api::get(tenant, &path).await?;
        parse_page(&body)
    }
}

fn parse_page(body: &Value) -> Result<Page> {
    let results = body
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("expected CREST page result array: {body}"),
        })?;
    let cookie = body
        .get("pagedResultsCookie")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.is_empty());
    Ok(Page { results, cookie })
}

fn required_id(record: &Value) -> Result<String> {
    record
        .get("_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("record has no string _id: {record}"),
        })
}

fn push_dedup(out: &mut Vec<Value>, seen: &mut HashSet<String>, records: Vec<Value>) {
    for record in records {
        match record.get("_id").and_then(Value::as_str) {
            Some(id) if seen.insert(id.to_string()) => out.push(record),
            Some(_) => {}
            None => out.push(record),
        }
    }
}

fn list_path(object: &str, params: &[(&str, &str)]) -> String {
    let mut query = Serializer::new(String::new());
    for (key, value) in params {
        query.append_pair(key, value);
    }
    format!(
        "/openidm/managed/{}?{}",
        path_segment(object),
        query.finish()
    )
}

fn path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn cursor_pages_walk_to_completion_and_dedupe() {
        let pages = vec![
            Page {
                results: vec![json!({"_id": "a"}), json!({"_id": "b"})],
                cookie: Some("next-1".into()),
            },
            Page {
                results: vec![json!({"_id": "b"}), json!({"_id": "c"})],
                cookie: Some("next-2".into()),
            },
            Page {
                results: vec![json!({"_id": "d"}), json!({"_id": "c"})],
                cookie: Some(String::new()),
            },
        ];
        let mut pages = pages.into_iter();
        let mut cookies = Vec::new();

        let records = collect_cursor_pages(|cookie| {
            cookies.push(cookie);
            let page = pages.next().expect("expected page");
            async move { Ok(page) }
        })
        .await
        .unwrap();

        assert_eq!(
            cookies,
            vec![None, Some("next-1".into()), Some("next-2".into())]
        );
        let ids = records
            .iter()
            .map(|v| v["_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["a", "b", "c", "d"]);
    }

    #[tokio::test]
    async fn cursor_pages_empty_first_page_returns_empty() {
        let mut calls = 0;

        let records = collect_cursor_pages(|cookie| {
            calls += 1;
            async move {
                assert!(cookie.is_none());
                Ok(Page {
                    results: Vec::new(),
                    cookie: None,
                })
            }
        })
        .await
        .unwrap();

        assert!(records.is_empty());
        assert_eq!(calls, 1);
    }
}
