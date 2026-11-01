//! Verified HTTP wrappers for the IDM `managed` config document.
//! All HTTP goes through `aic::api` → agent. See `docs/api/10-managed-objects.md`.

use crate::{Error, Result};
use serde_json::Value;

/// What a config write must be able to observe before it is believed.
///
/// `config/managed` is not read-your-writes consistent, so a 200 on the PUT is
/// not evidence the change is stored (Q14). Every writer states what it expects
/// to see and the write is retried until the tenant agrees.
#[derive(Debug)]
pub enum ConfigConfirm {
    /// The object exists and its content equals this exactly.
    ObjectContent { name: String, content: Value },
    /// The object exists; content unconstrained.
    ObjectPresent { name: String },
    /// No object of this name exists.
    ObjectAbsent { name: String },
    /// The whole document equals this.
    DocumentEquals(Value),
}

impl ConfigConfirm {
    /// Whether this confirmation condition holds for a fetched managed document.
    pub fn holds(&self, doc: &Value) -> bool {
        match self {
            Self::ObjectContent { name, content } => {
                object_named(doc, name).is_ok_and(|object| object_content_equal(object, content))
            }
            Self::ObjectPresent { name } => object_named(doc, name).is_ok(),
            Self::ObjectAbsent { name } => objects(doc).is_ok_and(|objects| {
                !objects
                    .iter()
                    .any(|object| object.get("name").and_then(Value::as_str) == Some(name))
            }),
            Self::DocumentEquals(expected) => doc == expected,
        }
    }

    fn description(&self) -> String {
        match self {
            Self::ObjectContent { name, .. } => {
                format!("managed object '{name}' with the requested content")
            }
            Self::ObjectPresent { name } => format!("managed object '{name}' to be present"),
            Self::ObjectAbsent { name } => format!("managed object '{name}' to be absent"),
            Self::DocumentEquals(_) => "the complete managed config document to match".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordCount {
    Exact(usize),
    AtLeast(usize),
}

impl std::fmt::Display for RecordCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(n) => write!(f, "{n}"),
            Self::AtLeast(n) => write!(f, "{n}+"),
        }
    }
}

/// Gets a bounded record count for a rename warning without exhausting pages.
pub async fn count_records(tenant: &str, object_name: &str) -> Result<RecordCount> {
    let path =
        format!("/openidm/managed/{object_name}?_queryFilter=true&_fields=_id&_pageSize=100");
    let body = crate::aic::api::get(tenant, &path).await?;
    let count = body
        .get("result")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if body
        .get("pagedResultsCookie")
        .is_some_and(|value| !value.is_null())
    {
        Ok(RecordCount::AtLeast(count))
    } else {
        Ok(RecordCount::Exact(count))
    }
}

/// `GET /openidm/config/managed` → the full schema document
/// (`{ _id: "managed", objects: [...] }`). No `_rev`.
pub async fn get_managed(tenant: &str) -> Result<Value> {
    crate::aic::api::get(tenant, "/openidm/config/managed").await
}

/// `GET /openidm/config/repo.ds` — the record-store mapping document
/// (`docs/api/10-managed-objects.md`, "Relationship storage").
pub async fn get_repo_ds(tenant: &str) -> Result<Value> {
    crate::aic::api::get(tenant, "/openidm/config/repo.ds").await
}

/// `PUT /openidm/config/repo.ds?waitForCompletion=true` — the whole document.
/// The single workflow is `repair::apply_with`, for removals and additions; a
/// [`WritePermit`](crate::managed::repair::WritePermit) is required to get here. A JSON
/// `PATCH` on this document was measured to return 200 and change nothing,
/// which is why there is no patch wrapper.
pub async fn put_repo_ds(
    tenant: &str,
    _permit: &crate::managed::repair::WritePermit,
    document: &Value,
    confirmed_prod: bool,
) -> Result<()> {
    crate::aic::api::put(
        tenant,
        "/openidm/config/repo.ds?waitForCompletion=true",
        document.clone(),
        confirmed_prod,
    )
    .await
    .map(|_| ())
}

/// One page of `object`'s records carrying only `_id` and the relationship
/// `key`. Paging records, not a `pr` filter: `<key> pr` matched a linked
/// record on a mapped property and nothing on a generic one (2026-10-26).
pub async fn records_with(
    tenant: &str,
    object: &str,
    key: &str,
    scan: usize,
) -> Result<Vec<Value>> {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("_queryFilter", "true")
        .append_pair("_pageSize", &scan.to_string())
        .append_pair("_fields", &format!("_id,{key}"));
    let path = format!(
        "/openidm/managed/{}?{}",
        crate::idmstore::api::path_segment(object),
        query.finish()
    );
    let body = get_settled(tenant, object, &path).await?;
    // A page without a `result` array is not a page of zero records: reading
    // it as one would report the pair "untested" on a response nothing
    // decoded (Sol round 6).
    body.get("result")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("managed/{object} query returned no `result` array: {body}"),
        })
}

/// What the far record's `key` says about `near_id`, as a record carrying
/// `key`; `None` when the far record does not exist. An array end is asked
/// through its relationship collection with `_refResourceId eq`, so a large
/// has-many is not read whole; a singular end's collection endpoint answered
/// 501, so the record is read with `_fields`. Measured 2026-10-26: the array
/// query in both storage modes, the singular 501 on a generic end only — the
/// mapped singular collection read was not tried.
pub async fn far_end(
    tenant: &str,
    object: &str,
    id: &str,
    key: &str,
    array: bool,
    near_id: &str,
) -> Result<Option<Value>> {
    let record = format!(
        "/openidm/managed/{}/{}",
        crate::idmstore::api::path_segment(object),
        crate::idmstore::api::path_segment(id)
    );
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    let path = if array {
        query
            .append_pair(
                "_queryFilter",
                &format!("_refResourceId eq \"{}\"", near_id.replace('"', "\\\"")),
            )
            .append_pair(
                "_fields",
                "_refResourceId,_refResourceCollection,_refProperties",
            );
        format!(
            "{record}/{}?{}",
            crate::idmstore::api::path_segment(key),
            query.finish()
        )
    } else {
        query.append_pair("_fields", key);
        format!("{record}?{}", query.finish())
    };
    match get_settled(tenant, object, &path).await {
        Ok(body) if array => {
            let result = body
                .get("result")
                .filter(|result| result.is_array())
                .cloned()
                .ok_or_else(|| Error::Api {
                    status: 0,
                    body: format!("{record}/{key} query returned no `result` array: {body}"),
                })?;
            // Measured 2026-10-31: the relationship collection of a record
            // that does not exist answers 200 with an empty result, not 404,
            // so an empty answer is only "lists nothing" once the record is
            // shown to exist. Without this a missing far record read as a
            // one-sided link, and relink removed a link it could not re-create.
            if result.as_array().is_some_and(Vec::is_empty)
                && record_with(tenant, object, id, "_id").await?.is_none()
            {
                return Ok(None);
            }
            Ok(Some(serde_json::json!({ "_id": id, key: result })))
        }
        Ok(body) => Ok(Some(body)),
        // Only a 404 the router did not give is "no such record": the router's
        // `Resource '…' not found` also answers for a relationship property
        // the object does not declare, which is a question about the schema.
        Err(Error::Api { status: 404, body }) if !collection_not_found(&body, object) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether `<object>/<id>/<key>?_queryFilter=/_ref eq "<reference>"` lists
/// the link — the filter callers reach for first. Measured 2026-10-31 on a
/// has-many end: a mapped pair answers it, a generic pair returns an empty
/// result for a link the collection lists (`docs/api/10-managed-objects.md`,
/// "Querying a relationship collection").
pub async fn ref_filter_finds(
    tenant: &str,
    object: &str,
    id: &str,
    key: &str,
    reference: &str,
) -> Result<bool> {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair(
            "_queryFilter",
            &format!("/_ref eq \"{}\"", reference.replace('"', "\\\"")),
        )
        .append_pair("_fields", "_ref");
    let path = format!(
        "/openidm/managed/{}/{}/{}?{}",
        crate::idmstore::api::path_segment(object),
        crate::idmstore::api::path_segment(id),
        crate::idmstore::api::path_segment(key),
        query.finish()
    );
    let body = get_settled(tenant, object, &path).await?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("{path} returned no `result` array: {body}"),
        })?;
    Ok(result
        .iter()
        .any(|entry| entry.get("_ref").and_then(Value::as_str) == Some(reference)))
}

/// Whether `managed/<object>?_queryFilter=_id eq "<id>" and <key> pr` finds
/// record `id` — on a record holding a link through `key`, true only when the
/// running IDM stores `key` mapped ([`crate::managed::verify::Runtime`]).
pub async fn present_filter_finds(tenant: &str, object: &str, id: &str, key: &str) -> Result<bool> {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair(
            "_queryFilter",
            &format!("_id eq \"{}\" and {key} pr", id.replace('"', "\\\"")),
        )
        .append_pair("_fields", "_id");
    let path = format!(
        "/openidm/managed/{}?{}",
        crate::idmstore::api::path_segment(object),
        query.finish()
    );
    let body = get_settled(tenant, object, &path).await?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("{path} returned no `result` array: {body}"),
        })?;
    Ok(result
        .iter()
        .any(|entry| entry.get("_id").and_then(Value::as_str) == Some(id)))
}

/// Finds evidence of orphaned values after assigning a reference column.
/// A single match is enough to fail: this is not a generic-link census.
pub async fn present_records(tenant: &str, object: &str, key: &str) -> Result<Vec<String>> {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("_queryFilter", &format!("/{key} pr"))
        .append_pair("_fields", "_id")
        .append_pair("_pageSize", "1");
    let path = format!(
        "/openidm/managed/{}?{}",
        crate::idmstore::api::path_segment(object),
        query.finish()
    );
    let body = get_settled(tenant, object, &path).await?;
    let records = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Config(format!("orphan query returned no result array: {body}")))?;
    records
        .iter()
        .map(|record| {
            record
                .get("_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| Error::Config("orphan query returned a record without _id".into()))
        })
        .collect()
}

/// One activation probe of `object.key` through record `id`:
/// `GET managed/<object>/<id>/<key>?_queryFilter=true&_pageSize=1`.
/// [`crate::managed::relink::activation`] judges the answer.
pub async fn probe_relationship(tenant: &str, object: &str, id: &str, key: &str) -> Result<Value> {
    let path = format!(
        "/openidm/managed/{}/{}/{}?_queryFilter=true&_pageSize=1&_fields=_ref",
        crate::idmstore::api::path_segment(object),
        crate::idmstore::api::path_segment(id),
        crate::idmstore::api::path_segment(key),
    );
    crate::aic::api::get(tenant, &path).await
}

/// The `_id` of any one record of `object`, or `None` when it has none.
pub async fn any_record_id(tenant: &str, object: &str) -> Result<Option<String>> {
    let path = format!(
        "/openidm/managed/{}?_queryFilter=true&_pageSize=1&_fields=_id",
        crate::idmstore::api::path_segment(object)
    );
    let body = get_settled(tenant, object, &path).await?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("managed/{object} query returned no `result` array: {body}"),
        })?;
    Ok(result
        .first()
        .and_then(|record| record.get("_id"))
        .and_then(Value::as_str)
        .map(str::to_string))
}

/// How long [`get_settled`] retries a collection's 404.
const SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// How long after this process's last `config/managed` write a router 404 is
/// read as settling. The measured settling lasted 0.6–1.1 s; outside the
/// window the same answer is a real one — a relationship property or object
/// that does not exist answers it too (`docs/api/10-managed-objects.md`) —
/// and retrying it would only delay the error by thirty seconds.
const SETTLE_WINDOW: std::time::Duration = std::time::Duration::from_secs(120);

static LAST_CONFIG_WRITE: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);

/// Opens the settling window. Called after every `config/managed` write,
/// failed ones included, because a write that errored may have landed.
fn note_config_write() {
    if let Ok(mut last) = LAST_CONFIG_WRITE.lock() {
        *last = Some(std::time::Instant::now());
    }
}

fn since_config_write() -> Option<std::time::Duration> {
    LAST_CONFIG_WRITE
        .lock()
        .ok()
        .and_then(|last| last.map(|at| at.elapsed()))
}

/// Whether a 404 with `body` is retried: only the router's answer for
/// `object`, only within [`SETTLE_WINDOW`] of a `config/managed` write this
/// process made, and only for [`SETTLE_TIMEOUT`].
fn retry_settling(
    body: &str,
    object: &str,
    since_write: Option<std::time::Duration>,
    waited: std::time::Duration,
) -> bool {
    since_write.is_some_and(|since| since < SETTLE_WINDOW)
        && waited < SETTLE_TIMEOUT
        && collection_not_found(body, object)
}

/// Whether a 404 is the settling one for `managed/<object>`: the router's
/// `Resource '<path>' not found` for the collection or anything under it.
/// Measured 2026-10-31: for some seconds after a `config/managed` write, reads
/// of the collection and of existing records — and a `PATCH` of one, which
/// wrote nothing — answered that, then answered normally, then 404 again. A
/// record that does not exist answers differently, `No Such Entry: The search
/// base entry '…' does not exist`, so it is never retried here.
pub fn collection_not_found(body: &str, object: &str) -> bool {
    let name = format!("managed/{object}");
    [
        format!("Resource &#39;{name}&#39;"),
        format!("Resource &#39;{name}/"),
        format!("Resource '{name}'"),
        format!("Resource '{name}/"),
    ]
    .iter()
    .any(|prefix| body.contains(prefix.as_str()))
}

/// `GET path`, retrying while the answer is the settling 404 for `object`
/// (see [`collection_not_found`]), so a caller never reads it as "no such
/// record". Says on stderr when it had to wait.
async fn get_settled(tenant: &str, object: &str, path: &str) -> Result<Value> {
    settled(object, || crate::aic::api::get(tenant, path)).await
}

/// Runs `call` until its answer is not the settling 404 for `object`, for up
/// to [`SETTLE_TIMEOUT`]. Also used for link writes: the router answered the
/// settling 404 without routing the request, so nothing was written (one
/// measured `PATCH`; the other write shapes are inferred from the router).
async fn settled<F, Fut>(object: &str, mut call: F) -> Result<Value>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    let started = std::time::Instant::now();
    let mut waited = false;
    loop {
        match call().await {
            Err(Error::Api { status: 404, body })
                if retry_settling(&body, object, since_config_write(), started.elapsed()) =>
            {
                waited = true;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            result => {
                if waited && result.is_ok() {
                    eprintln!(
                        "note: managed/{object} answered 404 for {:.1}s, the settling after a \
                         config/managed write; read again once it answered",
                        started.elapsed().as_secs_f64()
                    );
                }
                return result;
            }
        }
    }
}

/// Every record of `object` with only `_id` and `key`, paged to the end.
pub async fn all_records_with(tenant: &str, object: &str, key: &str) -> Result<Vec<Value>> {
    let fields = format!("_id,{key}");
    crate::idmstore::api::collect_cursor_pages(|cookie| {
        let fields = fields.clone();
        async move {
            let started = std::time::Instant::now();
            loop {
                match crate::idmstore::api::list_records_cursor_page(
                    tenant,
                    object,
                    cookie.as_deref(),
                    Some(&fields),
                )
                .await
                {
                    Err(Error::Api { status: 404, body })
                        if retry_settling(
                            &body,
                            object,
                            since_config_write(),
                            started.elapsed(),
                        ) =>
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                    result => return result,
                }
            }
        }
    })
    .await
}

/// Record `id` of `object` with only `_id` and `key`; `None` when it is gone.
pub async fn record_with(tenant: &str, object: &str, id: &str, key: &str) -> Result<Option<Value>> {
    let path = format!(
        "/openidm/managed/{}/{}?_fields=_id,{}",
        crate::idmstore::api::path_segment(object),
        crate::idmstore::api::path_segment(id),
        url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>()
    );
    match get_settled(tenant, object, &path).await {
        Ok(body) => Ok(Some(body)),
        // Only a 404 the router did not give is "no such record": the router's
        // `Resource '…' not found` also answers for a relationship property
        // the object does not declare, which is a question about the schema.
        Err(Error::Api { status: 404, body }) if !collection_not_found(&body, object) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Sends one of the writes [`crate::managed::relink::writes`] planned.
pub async fn send_relink(
    tenant: &str,
    object: &str,
    write: &crate::managed::relink::Write,
    confirmed_prod: bool,
) -> Result<Value> {
    if !matches!(write.method, "DELETE" | "POST" | "PATCH") {
        return Err(Error::Config(format!(
            "relink has no {} write",
            write.method
        )));
    }
    settled(object, || {
        let mut call = crate::aic::api::ApiCall::new(tenant, write.method, &write.path)
            .confirmed_prod(confirmed_prod);
        if let Some(body) = &write.body {
            call = call.body(body.clone());
        }
        if let Some(revision) = &write.if_match {
            call = call.if_match(revision);
        }
        call.send()
    })
    .await
}

/// Unconfirmed `PUT /openidm/config/managed` with the complete managed config
/// document.
///
/// The API has no object-level patch endpoint: callers must read, mutate the
/// intended `objects[]` entry, and send the whole `{ "_id": "managed", ... }`
/// envelope back. External callers should use [`replace_managed_confirmed`].
async fn replace_managed(tenant: &str, doc: Value, confirmed_prod: bool) -> Result<Value> {
    let result = crate::aic::api::put(tenant, "/openidm/config/managed", doc, confirmed_prod).await;
    note_config_write();
    result
}

/// Replaces a managed config document until every requested condition is visible.
///
/// A successful PUT can still be followed by a stale GET (Q14). Retry the exact
/// same whole document rather than rebuilding it from that stale read: a
/// relationship may span two objects, and a partial rebase could corrupt its
/// other end. This endpoint has no `If-Match` and is already last-writer-wins,
/// so resending the body is no more dangerous than the original PUT.
///
/// Before the first write, the document is checked against `config/repo.ds`
/// ([`verify::transition_refusal`](crate::managed::verify::transition_refusal)):
/// this is the one function every `config/managed` write goes through, so the
/// check here covers undo and every verb, not just those that ask first.
/// `base` is the document the caller built `doc` from; it and this
/// function's own read are both baselines, because under Q14 either can be
/// stale and a stale baseline must not excuse the write.
pub async fn replace_managed_confirmed(
    tenant: &str,
    base: &Value,
    doc: Value,
    expect: &[ConfigConfirm],
    confirmed_prod: bool,
) -> Result<()> {
    replace_managed_confirmed_with(
        base,
        doc,
        expect,
        || get_managed(tenant),
        || get_repo_ds(tenant),
        |doc| async move { replace_managed(tenant, doc, confirmed_prod).await.map(drop) },
        tenant,
    )
    .await
}

/// [`replace_managed_confirmed`] with its I/O injected, so a test can pin
/// that the repo.ds check runs, and refuses, before anything is sent. Plain
/// closures returning futures rather than `AsyncFn`: the TUI spawns these
/// writes, and an async closure's future cannot be proven `Send` there.
async fn replace_managed_confirmed_with<G, R, P>(
    base: &Value,
    doc: Value,
    expect: &[ConfigConfirm],
    mut get: impl FnMut() -> G,
    get_repo_ds: impl FnOnce() -> R,
    mut put: impl FnMut(Value) -> P,
    tenant: &str,
) -> Result<()>
where
    G: std::future::Future<Output = Result<Value>>,
    R: std::future::Future<Output = Result<Value>>,
    P: std::future::Future<Output = Result<()>>,
{
    if expect.is_empty() {
        return Err(Error::Config(
            "managed config write requires at least one confirmation condition".into(),
        ));
    }
    let current = get().await?;
    let baselines = [base, &current];
    if crate::managed::verify::needs_repo_ds(&baselines, &doc) {
        let repo_ds = get_repo_ds().await?;
        crate::managed::verify::transition_refusal(&baselines, &doc, &repo_ds)
            .map_err(Error::Config)?;
    }

    const ATTEMPTS: u32 = 6;
    let mut failed = None;
    for attempt in 0..ATTEMPTS {
        put(doc.clone()).await?;
        let fetched = get().await?;
        failed = expect.iter().find(|condition| !condition.holds(&fetched));
        if failed.is_none() {
            return Ok(());
        }
        if attempt + 1 < ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(500 * (1 << attempt))).await;
        }
    }

    let failed = failed.map_or_else(
        || "an unknown confirmation condition".to_string(),
        ConfigConfirm::description,
    );
    Err(Error::Config(format!(
        "managed config write for tenant '{tenant}' was accepted but not persisted: expected {failed}; \
         see scripts/experiment-managed-lost-updates.sh and docs/api/99-quirks-and-open-questions.md Q14"
    )))
}

/// The `objects` array of a managed document.
pub fn objects(doc: &Value) -> Result<&Vec<Value>> {
    doc.get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/managed shape: {doc}"),
        })
}

/// The mutable `objects` array of a managed document.
pub fn objects_mut(doc: &mut Value) -> Result<&mut Vec<Value>> {
    if doc.get("objects").and_then(Value::as_array).is_none() {
        return Err(Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/managed shape: {doc}"),
        });
    }
    Ok(doc
        .get_mut("objects")
        .and_then(Value::as_array_mut)
        .expect("checked objects array above"))
}

/// Find one object definition by `name`.
pub fn object_named<'a>(doc: &'a Value, name: &str) -> Result<&'a Value> {
    objects(doc)?
        .iter()
        .find(|o| o.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| Error::Config(format!("no managed object named '{name}' on this tenant")))
}

/// Find one mutable object definition by `name`.
pub fn object_named_mut<'a>(doc: &'a mut Value, name: &str) -> Result<&'a mut Value> {
    objects_mut(doc)?
        .iter_mut()
        .find(|o| o.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| Error::Config(format!("no managed object named '{name}' on this tenant")))
}

/// Fetch the full managed document and clone out the requested object.
pub async fn get_managed_with_object(tenant: &str, name: &str) -> Result<(Value, Value)> {
    let doc = get_managed(tenant).await?;
    let object = object_named(&doc, name)?.clone();
    Ok((doc, object))
}

/// Replace one `objects[]` entry in an already-fetched managed document,
/// preserving the full document envelope and every other object verbatim.
pub fn replace_object(doc: &mut Value, name: &str, object: Value) -> Result<()> {
    let slot = object_named_mut(doc, name)?;
    *slot = object;
    Ok(())
}

/// Content comparison for managed object subtrees. `serde_json::Value`
/// equality compares object maps independent of key insertion order, which is
/// exactly what we need for snapshot-based drift checks.
pub fn object_content_equal(a: &Value, b: &Value) -> bool {
    a == b
}

/// An editable hook: an object-valued property with a string `source` and a
/// `type` mentioning javascript. File-backed hooks (`file` instead of
/// `source`) fail this test by design.
///
/// This is the single predicate for "a hook `aic script pull
/// managed/<obj>.<hook>` can fetch". Callers that print a script reference
/// or that list/write hook sources must use this rather than restating the
/// shape, or `managed get` will print a reference to a script that
/// `script pull` then refuses.
pub fn is_inline_hook(value: &Value) -> bool {
    value.is_object() && value.get("source").is_some_and(Value::is_string) && javascript_type(value)
}

fn is_file_hook(value: &Value) -> bool {
    value.is_object() && value.get("file").is_some_and(Value::is_string) && javascript_type(value)
}

fn javascript_type(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t.contains("javascript"))
}

/// Summary row for `aic managed list`.
pub struct ObjectSummary {
    pub name: String,
    pub properties: usize,
    /// Inline-source hooks (syncable via `aic script`).
    pub hooks_inline: Vec<String>,
    /// File-backed hooks (server-side files — read-only markers).
    pub hooks_file: Vec<String>,
}

pub fn summarize(doc: &Value) -> Result<Vec<ObjectSummary>> {
    let mut out = Vec::new();
    for obj in objects(doc)? {
        let Some(name) = obj.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(map) = obj.as_object() else { continue };
        let properties = obj
            .pointer("/schema/properties")
            .and_then(Value::as_object)
            .map(|m| m.len())
            .unwrap_or(0);
        let mut hooks_inline = Vec::new();
        let mut hooks_file = Vec::new();
        for (key, value) in map {
            if key == "schema" {
                continue;
            }
            if is_inline_hook(value) {
                hooks_inline.push(key.clone());
            } else if is_file_hook(value) {
                hooks_file.push(key.clone());
            }
        }
        hooks_inline.sort();
        hooks_file.sort();
        out.push(ObjectSummary {
            name: name.to_string(),
            properties,
            hooks_inline,
            hooks_file,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(test)]
mod tests {

    /// The control the first version lacked: a permanently missing property
    /// answers the same router 404, so it is retried only just after a
    /// config write this process made.
    #[test]
    fn a_router_404_is_retried_only_just_after_a_config_write() {
        use std::time::Duration;
        let missing = "Resource &#39;managed/a/a1/noSuchRelationship&#39; not found";
        let zero = Duration::ZERO;
        assert!(!retry_settling(missing, "a", None, zero));
        assert!(retry_settling(
            missing,
            "a",
            Some(Duration::from_secs(1)),
            zero
        ));
        assert!(!retry_settling(missing, "a", Some(SETTLE_WINDOW), zero));
        assert!(!retry_settling(
            missing,
            "a",
            Some(Duration::from_secs(1)),
            SETTLE_TIMEOUT
        ));
        let no_record = "No Such Entry: The search base entry &#39;uid&#61;x&#39; does not exist";
        assert!(!retry_settling(
            no_record,
            "a",
            Some(Duration::from_secs(1)),
            zero
        ));
    }

    #[test]
    fn only_a_404_naming_the_collection_is_the_settling_one() {
        let measured = r#"{"code":404,"reason":"Not Found","message":"Resource &#39;managed/zzevalqb&#39; not found"}"#;
        assert!(collection_not_found(measured, "zzevalqb"));
        assert!(collection_not_found(
            "Resource 'managed/zzevalqb' not found",
            "zzevalqb"
        ));
        // The same router answer for an existing record, measured on a PATCH.
        assert!(collection_not_found(
            "Resource &#39;managed/zzevalqa/a1&#39; not found",
            "zzevalqa"
        ));
        // A record that does not exist, and another object's 404, are answers.
        assert!(!collection_not_found(
            "No Such Entry: The search base entry &#39;uid&#61;nosuch,ou&#61;zzevalqb,ou&#61;managed,dc&#61;openidm&#39; does not exist",
            "zzevalqb"
        ));
        assert!(!collection_not_found(
            "Resource &#39;managed/zzevalqbx&#39; not found",
            "zzevalqb"
        ));
    }
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_detection_is_value_shaped() {
        assert!(is_inline_hook(
            &json!({"type": "text/javascript", "source": "x"})
        ));
        // File-backed: read-only, must not be detected as editable.
        assert!(!is_inline_hook(
            &json!({"type": "text/javascript", "file": "roles/onDelete-roles.js"})
        ));
        assert!(!is_inline_hook(&json!({"source": "x"})));
        assert!(!is_inline_hook(&json!("text")));
    }

    #[test]
    fn summarize_classifies_hooks_and_counts_properties() {
        let doc = json!({"objects": [{
            "name": "alpha_user",
            "schema": {"properties": {"a": {}, "b": {}}},
            "onCreate": {"type": "text/javascript", "source": "x"},
            "onDelete": {"type": "text/javascript", "file": "roles/onDelete-roles.js"},
            "iconClass": "fa fa-user"
        }]});
        let s = summarize(&doc).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "alpha_user");
        assert_eq!(s[0].properties, 2);
        assert_eq!(s[0].hooks_inline, vec!["onCreate"]);
        assert_eq!(s[0].hooks_file, vec!["onDelete"]);
    }

    #[test]
    fn object_named_errors_helpfully() {
        let doc = json!({"objects": []});
        assert!(
            object_named(&doc, "ghost")
                .unwrap_err()
                .to_string()
                .contains("ghost")
        );
    }

    #[test]
    fn replace_object_preserves_document_envelope() {
        let mut doc = json!({
            "_id": "managed",
            "objects": [
                {"name": "alpha_user", "schema": {"properties": {"a": {}}}},
                {"name": "alpha_role", "schema": {"properties": {}}}
            ],
            "extra": true
        });

        replace_object(
            &mut doc,
            "alpha_user",
            json!({"name": "alpha_user", "schema": {"properties": {"b": {}}}}),
        )
        .unwrap();

        assert_eq!(doc["_id"], json!("managed"));
        assert_eq!(doc["extra"], json!(true));
        assert_eq!(doc["objects"][0]["schema"]["properties"], json!({"b": {}}));
        assert_eq!(doc["objects"][1]["name"], json!("alpha_role"));
    }

    #[test]
    fn object_content_equal_ignores_map_insertion_order() {
        let a = json!({"name": "alpha_user", "schema": {"required": ["a"], "properties": {}}});
        let b = json!({"schema": {"properties": {}, "required": ["a"]}, "name": "alpha_user"});
        assert!(object_content_equal(&a, &b));
    }

    #[test]
    fn config_confirm_object_content_is_order_independent_but_value_sensitive() {
        let doc = json!({"objects": [{"name": "alpha_user", "schema": {"properties": {"a": 1}}}]});
        assert!(
            ConfigConfirm::ObjectContent {
                name: "alpha_user".into(),
                content: json!({"schema": {"properties": {"a": 1}}, "name": "alpha_user"}),
            }
            .holds(&doc)
        );
        assert!(
            !ConfigConfirm::ObjectContent {
                name: "alpha_user".into(),
                content: json!({"name": "alpha_user", "schema": {"properties": {"a": 2}}}),
            }
            .holds(&doc)
        );
    }

    #[test]
    fn config_confirm_object_presence_and_absence() {
        let doc = json!({"objects": [{"name": "alpha_user"}]});
        assert!(
            ConfigConfirm::ObjectPresent {
                name: "alpha_user".into()
            }
            .holds(&doc)
        );
        assert!(
            !ConfigConfirm::ObjectPresent {
                name: "alpha_role".into()
            }
            .holds(&doc)
        );
        assert!(
            !ConfigConfirm::ObjectAbsent {
                name: "alpha_user".into()
            }
            .holds(&doc)
        );
        assert!(
            ConfigConfirm::ObjectAbsent {
                name: "alpha_role".into()
            }
            .holds(&doc)
        );
    }

    #[test]
    fn config_confirm_document_equals_distinguishes_documents() {
        let doc = json!({"objects": []});
        assert!(ConfigConfirm::DocumentEquals(json!({"objects": []})).holds(&doc));
        assert!(!ConfigConfirm::DocumentEquals(json!({"objects": [{}]})).holds(&doc));
    }

    /// The boundary check is wired, and runs before the first `PUT`: a write
    /// contradicting repo.ds sends nothing, and the control — the same write
    /// with no mapping — is sent.
    #[tokio::test]
    async fn the_repo_ds_check_refuses_before_anything_is_sent() {
        let before = json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {}}}
        ]});
        let after = json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {"owner": {"type": "string"}}}}
        ]});
        let mapping = json!({"resourceMapping": {"genericMapping": {"managed/a": {"properties": {
            "owner": {"type": "reference", "resourcePath": "managed/b"}}}}}});
        for (repo_ds, refused) in [(mapping, true), (json!({}), false)] {
            let puts = std::cell::Cell::new(0);
            let stored = std::cell::RefCell::new(before.clone());
            let result = replace_managed_confirmed_with(
                &before,
                after.clone(),
                &[ConfigConfirm::DocumentEquals(after.clone())],
                || std::future::ready(Ok(stored.borrow().clone())),
                || std::future::ready(Ok(repo_ds.clone())),
                |doc| {
                    puts.set(puts.get() + 1);
                    *stored.borrow_mut() = doc;
                    std::future::ready(Ok(()))
                },
                "t",
            )
            .await;
            assert_eq!(result.is_err(), refused, "{result:?}");
            assert_eq!(puts.get(), usize::from(!refused));
        }
    }

    /// Astra round 7, finding 3, through the real boundary: the boundary's
    /// own read is stale and already holds the proposed contradiction; the
    /// caller's baseline does not. Nothing is sent. The control passes the
    /// stale read as the caller's baseline too, and the write goes out.
    #[tokio::test]
    async fn a_stale_read_holding_the_proposal_does_not_excuse_it() {
        let before = json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {}}}
        ]});
        let after = json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {"owner": {"type": "string"}}}}
        ]});
        let mapping = json!({"resourceMapping": {"genericMapping": {"managed/a": {"properties": {
            "owner": {"type": "reference", "resourcePath": "managed/b"}}}}}});
        for (base, refused) in [(&before, true), (&after, false)] {
            let puts = std::cell::Cell::new(0);
            let result = replace_managed_confirmed_with(
                base,
                after.clone(),
                &[ConfigConfirm::DocumentEquals(after.clone())],
                || std::future::ready(Ok(after.clone())),
                || std::future::ready(Ok(mapping.clone())),
                |_| {
                    puts.set(puts.get() + 1);
                    std::future::ready(Ok(()))
                },
                "t",
            )
            .await;
            assert_eq!(result.is_err(), refused, "{result:?}");
            assert_eq!(puts.get(), usize::from(!refused));
        }
    }

    /// Sol/Astra round 9, finding 2: an object delete that only narrows a
    /// relationship already mismatched against its mapping is sent — the
    /// real `delete_object_in_doc` output, through the real boundary. The
    /// control adds a target instead, and nothing is sent.
    #[tokio::test]
    async fn narrowing_an_existing_mismatch_is_sent_and_widening_is_not() {
        let doc = |targets: &[&str]| {
            let collection: Vec<Value> = targets
                .iter()
                .map(|target| json!({"path": format!("managed/{target}")}))
                .collect();
            json!({"_id": "managed", "objects": [
                {"name": "a", "schema": {"properties": {
                    "owner": {"type": "relationship", "resourceCollection": collection}
                }}},
                {"name": "c", "schema": {"properties": {}}},
                {"name": "e", "schema": {"properties": {}}},
                {"name": "f", "schema": {"properties": {}}}
            ]})
        };
        let before = doc(&["c", "e"]);
        let (deleted, _) = crate::managed::ops::delete_object_in_doc(&before, "e").unwrap();
        assert_eq!(
            deleted["objects"][0]["schema"]["properties"]["owner"]["resourceCollection"],
            json!([{"path": "managed/c"}])
        );
        let mapping = json!({"resourceMapping": {"genericMapping": {"managed/a": {"properties": {
            "owner": {"type": "reference", "resourcePath": "managed/d"}}}}}});
        for (after, refused) in [(deleted, false), (doc(&["c", "e", "f"]), true)] {
            let puts = std::cell::Cell::new(0);
            let stored = std::cell::RefCell::new(before.clone());
            let result = replace_managed_confirmed_with(
                &before,
                after.clone(),
                &[ConfigConfirm::DocumentEquals(after.clone())],
                || std::future::ready(Ok(stored.borrow().clone())),
                || std::future::ready(Ok(mapping.clone())),
                |doc| {
                    puts.set(puts.get() + 1);
                    *stored.borrow_mut() = doc;
                    std::future::ready(Ok(()))
                },
                "t",
            )
            .await;
            assert_eq!(result.is_err(), refused, "{result:?}");
            assert_eq!(puts.get(), usize::from(!refused));
        }
    }

    #[tokio::test]
    async fn confirmed_replace_rejects_empty_expectations_before_network_io() {
        let error = replace_managed_confirmed("unused", &json!({}), json!({}), &[], false)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Config(message) if message.contains("confirmation condition"))
        );
    }
}
