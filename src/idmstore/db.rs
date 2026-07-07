//! SQLite storage for shredded IDM managed-object records.
//!
//! File map: this module owns connection setup, schema derivation, sync-state
//! watermark CRUD, record helpers, upsert/shredding logic, and SQL quoting /
//! column inference utilities.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params_from_iter};
use serde_json::{Map, Value};

pub type ArrayColumnOverrides = HashMap<String, Vec<ColumnSpec>>;
pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid IDM schema: {0}")]
    InvalidSchema(String),
    #[error("invalid IDM record: {0}")]
    InvalidRecord(String),
}

impl From<DbError> for crate::Error {
    fn from(error: DbError) -> Self {
        crate::Error::Config(format!("IDM store error: {error}"))
    }
}

// ── connection / pragmas / errors ────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncState {
    pub object: String,
    pub incremental_supported: bool,
    pub watermark: Option<String>,
    pub last_full_sync: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Text,
    Real,
    Integer,
}

impl ColumnType {
    fn sql(self) -> &'static str {
        match self {
            ColumnType::Text => "TEXT",
            ColumnType::Real => "REAL",
            ColumnType::Integer => "INTEGER",
        }
    }
}

// ── IDM schema → DDL derivation ──────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSpec {
    pub name: String,
    pub kind: ColumnType,
}

impl ColumnSpec {
    pub fn new(name: impl Into<String>, kind: ColumnType) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }

    pub fn text(name: impl Into<String>) -> Self {
        Self::new(name, ColumnType::Text)
    }

    pub fn real(name: impl Into<String>) -> Self {
        Self::new(name, ColumnType::Real)
    }

    pub fn integer(name: impl Into<String>) -> Self {
        Self::new(name, ColumnType::Integer)
    }
}

// ── sync-state watermark CRUD ────────────────────────────────────────────
// (functions below)

// ── record helpers (id/meta/delete/count) ───────────────────────────────
// (functions below)

// ── upsert + child-row shredding ────────────────────────────────────────
// (functions below)

// ── column inference / SQL quoting utils ────────────────────────────────
// (functions below)

#[derive(Debug, Clone)]
pub struct ObjectStore {
    base_table: String,
    generated: Vec<GeneratedColumn>,
    child_tables: Vec<ChildTable>,
}

impl ObjectStore {
    pub fn new(object: &str, schema: &Value, overrides: &ArrayColumnOverrides) -> Result<Self> {
        let props = schema_properties(schema).ok_or_else(|| {
            DbError::InvalidSchema(format!("object {object} has no schema.properties object"))
        })?;
        let mut generated = Vec::new();
        let mut child_tables = Vec::new();
        let mut prop_entries: Vec<_> = props.iter().collect();
        prop_entries.sort_by(|a, b| a.0.cmp(b.0));

        for (prop, def) in prop_entries {
            if let Some(kind) = scalar_column_type(def) {
                if !BASE_COLUMNS.contains(&prop.as_str()) {
                    generated.push(GeneratedColumn {
                        name: prop.clone(),
                        kind,
                        searchable: def
                            .get("searchable")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    });
                }
            }

            if let Some(child) = ChildTable::from_property(object, prop, def, overrides.get(prop)) {
                child_tables.push(child);
            }
        }

        Ok(Self {
            base_table: base_table(object),
            generated,
            child_tables,
        })
    }

    pub fn ddl(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(self.base_ddl());
        for col in &self.generated {
            if col.searchable {
                out.push(index_ddl(&self.base_table, &col.name));
            }
        }
        for child in &self.child_tables {
            out.push(child.ddl(&self.base_table));
            out.extend(child.indexes());
        }
        out
    }

    fn base_ddl(&self) -> String {
        let mut cols = vec![
            format!("{} TEXT PRIMARY KEY", qid("_id")),
            format!("{} TEXT NOT NULL", qid("data")),
            format!("{} TEXT", qid("rev")),
            format!("{} TEXT", qid("meta_id")),
            format!("{} TEXT", qid("meta_changed")),
            format!("{} TEXT", qid("synced_at")),
        ];
        cols.extend(self.generated.iter().map(|col| {
            format!(
                "{} {} GENERATED ALWAYS AS (data->>{}) VIRTUAL",
                qid(&col.name),
                col.kind.sql(),
                sql_string(&json_path(&col.name))
            )
        }));
        format!(
            "CREATE TABLE IF NOT EXISTS {} ({});",
            qid(&self.base_table),
            cols.join(", ")
        )
    }
}

#[derive(Debug, Clone)]
struct GeneratedColumn {
    name: String,
    kind: ColumnType,
    searchable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildSource {
    Array,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildShape {
    Scalar,
    Relationship,
    Object,
    Json,
}

#[derive(Debug, Clone)]
struct ChildTable {
    prop: String,
    table: String,
    source: ChildSource,
    shape: ChildShape,
    columns: Vec<ColumnSpec>,
    index_columns: Vec<String>,
}

impl ChildTable {
    fn from_property(
        object: &str,
        prop: &str,
        def: &Value,
        overrides: Option<&Vec<ColumnSpec>>,
    ) -> Option<Self> {
        if crate::managed::state::is_relationship_property(def) {
            let mut columns = vec![ColumnSpec::text("ref_path"), ColumnSpec::text("ref_id")];
            columns.extend(ref_property_columns(def));
            let columns = dedupe_columns(columns, &["parent_id", "idx"]);
            let index_columns = columns.iter().map(|col| col.name.clone()).collect();
            return Some(Self {
                prop: prop.to_string(),
                table: child_table(object, prop),
                source: if type_name(def) == Some("array") {
                    ChildSource::Array
                } else {
                    ChildSource::Single
                },
                shape: ChildShape::Relationship,
                columns,
                index_columns,
            });
        }

        if type_name(def) != Some("array") {
            return None;
        }

        let items = def.get("items").unwrap_or(&Value::Null);
        let item_type = type_name(items);
        let override_columns = overrides.cloned().unwrap_or_default();
        if !override_columns.is_empty() && item_type == Some("string") {
            return Some(Self::json(object, prop, override_columns));
        }

        match item_type {
            Some("string") | Some("number") | Some("integer") | Some("boolean") => {
                let kind = scalar_column_type(items).unwrap_or(ColumnType::Text);
                Some(Self {
                    prop: prop.to_string(),
                    table: child_table(object, prop),
                    source: ChildSource::Array,
                    shape: ChildShape::Scalar,
                    columns: vec![ColumnSpec::new("value", kind)],
                    index_columns: vec!["value".to_string()],
                })
            }
            Some("object") => {
                let declared = object_item_columns(items);
                if declared.is_empty() {
                    Some(Self::json(object, prop, override_columns))
                } else {
                    let columns = dedupe_columns(declared, &["parent_id", "idx"]);
                    let index_columns = columns.iter().map(|col| col.name.clone()).collect();
                    Some(Self {
                        prop: prop.to_string(),
                        table: child_table(object, prop),
                        source: ChildSource::Array,
                        shape: ChildShape::Object,
                        columns,
                        index_columns,
                    })
                }
            }
            _ => Some(Self::json(object, prop, override_columns)),
        }
    }

    fn json(object: &str, prop: &str, overrides: Vec<ColumnSpec>) -> Self {
        let mut columns = vec![ColumnSpec::text("elem")];
        columns.extend(overrides);
        let columns = dedupe_columns(columns, &["parent_id", "idx"]);
        let index_columns = columns
            .iter()
            .filter(|col| col.name != "elem")
            .map(|col| col.name.clone())
            .collect();
        Self {
            prop: prop.to_string(),
            table: child_table(object, prop),
            source: ChildSource::Array,
            shape: ChildShape::Json,
            columns,
            index_columns,
        }
    }

    fn ddl(&self, base_table: &str) -> String {
        let mut cols = vec![
            format!(
                "{} TEXT NOT NULL REFERENCES {}({}) ON DELETE CASCADE",
                qid("parent_id"),
                qid(base_table),
                qid("_id")
            ),
            format!("{} INTEGER NOT NULL", qid("idx")),
        ];
        cols.extend(
            self.columns
                .iter()
                .map(|col| format!("{} {}", qid(&col.name), col.kind.sql())),
        );
        cols.push(format!("PRIMARY KEY({}, {})", qid("parent_id"), qid("idx")));
        format!(
            "CREATE TABLE IF NOT EXISTS {} ({});",
            qid(&self.table),
            cols.join(", ")
        )
    }

    fn indexes(&self) -> Vec<String> {
        self.index_columns
            .iter()
            .map(|col| index_ddl(&self.table, col))
            .collect()
    }
}

const BASE_COLUMNS: &[&str] = &["_id", "data", "rev", "meta_id", "meta_changed", "synced_at"];

pub fn open(path: impl AsRef<Path>) -> Result<Connection> {
    let conn = Connection::open(path)?;
    init(&conn)?;
    Ok(conn)
}

pub fn open_readonly(path: impl AsRef<Path>) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

pub fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=30000;
         CREATE TABLE IF NOT EXISTS sync_state (
             object TEXT PRIMARY KEY,
             incremental_supported INTEGER NOT NULL,
             watermark TEXT,
             last_full_sync TEXT
         );",
    )?;
    Ok(())
}

pub fn create_schema(conn: &Connection, store: &ObjectStore) -> Result<()> {
    for stmt in store.ddl() {
        conn.execute_batch(&stmt)?;
    }
    Ok(())
}

pub fn read_sync_state(conn: &Connection, object: &str) -> Result<Option<SyncState>> {
    if !table_exists(conn, "sync_state")? {
        return Ok(None);
    }
    conn.query_row(
        "SELECT object, incremental_supported, watermark, last_full_sync \
         FROM sync_state WHERE object = ?",
        [object],
        |row| {
            Ok(SyncState {
                object: row.get(0)?,
                incremental_supported: row.get::<_, i64>(1)? != 0,
                watermark: row.get(2)?,
                last_full_sync: row.get(3)?,
            })
        },
    )
    .optional()
    .map_err(DbError::from)
}

pub fn write_sync_state(conn: &Connection, state: &SyncState) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (object, incremental_supported, watermark, last_full_sync) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(object) DO UPDATE SET \
             incremental_supported=excluded.incremental_supported, \
             watermark=excluded.watermark, \
             last_full_sync=excluded.last_full_sync",
        rusqlite::params![
            state.object.as_str(),
            i64::from(state.incremental_supported),
            state.watermark.as_deref(),
            state.last_full_sync.as_deref()
        ],
    )?;
    Ok(())
}

pub fn list_sync_states(conn: &Connection) -> Result<Vec<SyncState>> {
    if !table_exists(conn, "sync_state")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT object, incremental_supported, watermark, last_full_sync \
         FROM sync_state ORDER BY object",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SyncState {
            object: row.get(0)?,
            incremental_supported: row.get::<_, i64>(1)? != 0,
            watermark: row.get(2)?,
            last_full_sync: row.get(3)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(DbError::from)
}

pub fn local_ids(conn: &Connection, object: &str) -> Result<BTreeSet<String>> {
    let table = base_table(object);
    if !table_exists(conn, &table)? {
        return Ok(BTreeSet::new());
    }
    let sql = format!(
        "SELECT {} FROM {} ORDER BY {}",
        qid("_id"),
        qid(&table),
        qid("_id")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<BTreeSet<_>, _>>()
        .map_err(DbError::from)
}

pub fn record_ids_for_meta_ids(
    conn: &Connection,
    object: &str,
    meta_ids: &[String],
) -> Result<HashMap<String, String>> {
    let table = base_table(object);
    if meta_ids.is_empty() || !table_exists(conn, &table)? {
        return Ok(HashMap::new());
    }

    let mut out = HashMap::new();
    for chunk in meta_ids.chunks(900) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {}, {} FROM {} WHERE {} IN ({})",
            qid("meta_id"),
            qid("_id"),
            qid(&table),
            qid("meta_id"),
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk.iter().map(String::as_str)), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (meta_id, id) = row?;
            out.insert(meta_id, id);
        }
    }
    Ok(out)
}

pub fn delete_records_by_id(conn: &mut Connection, object: &str, ids: &[String]) -> Result<usize> {
    let table = base_table(object);
    if ids.is_empty() || !table_exists(conn, &table)? {
        return Ok(0);
    }

    let tx = conn.transaction()?;
    let sql = format!("DELETE FROM {} WHERE {} = ?", qid(&table), qid("_id"));
    let mut count = 0;
    for id in ids {
        count += tx.execute(&sql, [id])?;
    }
    tx.commit()?;
    Ok(count)
}

pub fn object_row_count(conn: &Connection, object: &str) -> Result<usize> {
    let table = base_table(object);
    if !table_exists(conn, &table)? {
        return Ok(0);
    }
    let sql = format!("SELECT COUNT(*) FROM {}", qid(&table));
    let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
    Ok(usize::try_from(count).unwrap_or(0))
}

pub fn upsert_many<'a>(
    conn: &mut Connection,
    object: &ObjectStore,
    records: impl IntoIterator<Item = &'a Value>,
) -> Result<()> {
    let tx = conn.transaction()?;
    for record in records {
        upsert_in_tx(&tx, object, record)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn infer_columns(elements: &[Value]) -> Vec<ColumnSpec> {
    let mut found: BTreeMap<String, ColumnType> = BTreeMap::new();
    for element in elements {
        let parsed = parse_stringified_json(element);
        let source = parsed.as_ref().unwrap_or(element);
        let Some(object) = source.as_object() else {
            continue;
        };
        for (key, value) in object {
            let Some(kind) = inferred_type(value) else {
                continue;
            };
            found
                .entry(key.clone())
                .and_modify(|existing| *existing = merge_column_types(*existing, kind))
                .or_insert(kind);
        }
    }
    found
        .into_iter()
        .map(|(name, kind)| ColumnSpec::new(name, kind))
        .collect()
}

fn upsert_in_tx(tx: &Transaction<'_>, object: &ObjectStore, record: &Value) -> Result<()> {
    let id = record
        .get("_id")
        .and_then(Value::as_str)
        .ok_or_else(|| DbError::InvalidRecord("record has no string _id".into()))?;
    let data = serde_json::to_string(record)?;
    let rev = record.get("_rev").and_then(scalar_string);
    let meta_id = record
        .pointer("/_meta/_refResourceId")
        .and_then(scalar_string)
        .or_else(|| record.pointer("/_meta/_id").and_then(scalar_string));
    let meta_changed = record
        .pointer("/_meta/lastChanged/date")
        .and_then(scalar_string);

    let sql = format!(
        "INSERT INTO {} ({}, {}, {}, {}, {}, {}) VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ','now')) \
         ON CONFLICT({}) DO UPDATE SET {}=excluded.{}, {}=excluded.{}, {}=excluded.{}, {}=excluded.{}, {}=excluded.{}",
        qid(&object.base_table),
        qid("_id"),
        qid("data"),
        qid("rev"),
        qid("meta_id"),
        qid("meta_changed"),
        qid("synced_at"),
        qid("_id"),
        qid("data"),
        qid("data"),
        qid("rev"),
        qid("rev"),
        qid("meta_id"),
        qid("meta_id"),
        qid("meta_changed"),
        qid("meta_changed"),
        qid("synced_at"),
        qid("synced_at"),
    );
    tx.execute(
        &sql,
        rusqlite::params![id, data, rev, meta_id, meta_changed],
    )?;

    for child in &object.child_tables {
        let delete = format!(
            "DELETE FROM {} WHERE {} = ?",
            qid(&child.table),
            qid("parent_id")
        );
        tx.execute(&delete, [id])?;
        insert_child_rows(tx, child, id, record)?;
    }

    Ok(())
}

fn insert_child_rows(
    tx: &Transaction<'_>,
    child: &ChildTable,
    parent_id: &str,
    record: &Value,
) -> Result<()> {
    let Some(value) = record.get(&child.prop) else {
        return Ok(());
    };
    let mut cols = vec!["parent_id".to_string(), "idx".to_string()];
    cols.extend(child.columns.iter().map(|col| col.name.clone()));
    let placeholders = std::iter::repeat_n("?", cols.len())
        .collect::<Vec<_>>()
        .join(", ");
    let col_sql = cols
        .iter()
        .map(|col| qid(col))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        qid(&child.table),
        col_sql,
        placeholders
    );

    match child.source {
        ChildSource::Array => {
            let Some(items) = value.as_array() else {
                return Ok(());
            };
            for (idx, element) in items.iter().enumerate() {
                insert_child_row(tx, child, &sql, parent_id, idx, element)?;
            }
        }
        ChildSource::Single => {
            if !value.is_null() {
                insert_child_row(tx, child, &sql, parent_id, 0, value)?;
            }
        }
    }
    Ok(())
}

fn insert_child_row(
    tx: &Transaction<'_>,
    child: &ChildTable,
    sql: &str,
    parent_id: &str,
    idx: usize,
    element: &Value,
) -> Result<()> {
    let mut values = vec![
        SqlValue::Text(parent_id.to_string()),
        SqlValue::Integer(idx as i64),
    ];
    values.extend(child_values(child, element)?);
    tx.execute(sql, params_from_iter(values.iter()))?;
    Ok(())
}

fn child_values(child: &ChildTable, element: &Value) -> Result<Vec<SqlValue>> {
    match child.shape {
        ChildShape::Scalar => Ok(vec![to_sql_value(Some(element), child.columns[0].kind)?]),
        ChildShape::Relationship => relationship_values(child, element),
        ChildShape::Object => object_values(child, element),
        ChildShape::Json => json_values(child, element),
    }
}

fn relationship_values(child: &ChildTable, element: &Value) -> Result<Vec<SqlValue>> {
    let mut values = Vec::with_capacity(child.columns.len());
    let (path, id) = relationship_ref(element);
    values.push(path.map(SqlValue::Text).unwrap_or(SqlValue::Null));
    values.push(id.map(SqlValue::Text).unwrap_or(SqlValue::Null));

    let ref_props = element
        .as_object()
        .and_then(|object| object.get("_refProperties"))
        .and_then(Value::as_object);
    for col in child.columns.iter().skip(2) {
        values.push(to_sql_value(
            ref_props.and_then(|props| props.get(&col.name)),
            col.kind,
        )?);
    }
    Ok(values)
}

fn object_values(child: &ChildTable, element: &Value) -> Result<Vec<SqlValue>> {
    let parsed = parse_stringified_json(element);
    let source = parsed.as_ref().unwrap_or(element);
    let object = source.as_object();
    child
        .columns
        .iter()
        .map(|col| to_sql_value(object.and_then(|object| object.get(&col.name)), col.kind))
        .collect()
}

fn json_values(child: &ChildTable, element: &Value) -> Result<Vec<SqlValue>> {
    let parsed = parse_stringified_json(element);
    let source = parsed.as_ref().unwrap_or(element);
    let object = source.as_object();
    child
        .columns
        .iter()
        .map(|col| {
            if col.name == "elem" {
                element_json_text(element).map(SqlValue::Text)
            } else {
                to_sql_value(object.and_then(|object| object.get(&col.name)), col.kind)
            }
        })
        .collect()
}

fn relationship_ref(element: &Value) -> (Option<String>, Option<String>) {
    let raw = match element {
        Value::String(s) => Some(s.as_str()),
        Value::Object(object) => object.get("_ref").and_then(Value::as_str),
        _ => None,
    };
    let Some(raw) = raw else {
        return (None, None);
    };
    match raw.rsplit_once('/') {
        Some((path, id)) => (Some(path.to_string()), Some(id.to_string())),
        None => (None, Some(raw.to_string())),
    }
}

fn to_sql_value(value: Option<&Value>, kind: ColumnType) -> Result<SqlValue> {
    let Some(value) = value else {
        return Ok(SqlValue::Null);
    };
    if value.is_null() {
        return Ok(SqlValue::Null);
    }
    Ok(match kind {
        ColumnType::Text => SqlValue::Text(text_value(value)?),
        ColumnType::Real => value
            .as_f64()
            .map(SqlValue::Real)
            .or_else(|| value.as_str()?.parse::<f64>().ok().map(SqlValue::Real))
            .unwrap_or(SqlValue::Null),
        ColumnType::Integer => {
            if let Some(value) = value.as_bool() {
                SqlValue::Integer(i64::from(value))
            } else if let Some(value) = value.as_i64() {
                SqlValue::Integer(value)
            } else {
                value
                    .as_str()
                    .and_then(|s| s.parse::<i64>().ok())
                    .map(SqlValue::Integer)
                    .unwrap_or(SqlValue::Null)
            }
        }
    })
}

fn text_value(value: &Value) -> Result<String> {
    Ok(match value {
        Value::String(s) => s.clone(),
        Value::Number(_) | Value::Bool(_) => value.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value)?,
        Value::Null => String::new(),
    })
}

fn element_json_text(element: &Value) -> Result<String> {
    if let Value::String(s) = element {
        if serde_json::from_str::<Value>(s).is_ok() {
            return Ok(s.clone());
        }
    }
    Ok(serde_json::to_string(element)?)
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        _ => None,
    }
}

fn schema_properties(schema: &Value) -> Option<&Map<String, Value>> {
    schema
        .pointer("/schema/properties")
        .and_then(Value::as_object)
        .or_else(|| schema.pointer("/properties").and_then(Value::as_object))
}

fn type_name(value: &Value) -> Option<&str> {
    match value.get("type") {
        Some(Value::String(kind)) => Some(kind.as_str()),
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(Value::as_str)
            .find(|kind| *kind != "null"),
        _ => None,
    }
}

fn scalar_column_type(value: &Value) -> Option<ColumnType> {
    match type_name(value)? {
        "string" => Some(ColumnType::Text),
        "number" => Some(ColumnType::Real),
        "integer" | "boolean" => Some(ColumnType::Integer),
        _ => None,
    }
}

fn object_item_columns(items: &Value) -> Vec<ColumnSpec> {
    let Some(props) = items.get("properties").and_then(Value::as_object) else {
        return Vec::new();
    };
    columns_from_properties(props)
}

fn ref_property_columns(property: &Value) -> Vec<ColumnSpec> {
    let mut out = Vec::new();
    for pointer in [
        "/_refProperties/properties",
        "/items/_refProperties/properties",
        "/properties/_refProperties/properties",
        "/items/properties/_refProperties/properties",
    ] {
        if let Some(props) = property.pointer(pointer).and_then(Value::as_object) {
            out.extend(columns_from_properties(props));
        }
    }
    dedupe_columns(out, &["parent_id", "idx", "ref_path", "ref_id"])
}

fn columns_from_properties(props: &Map<String, Value>) -> Vec<ColumnSpec> {
    let mut entries: Vec<_> = props.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .into_iter()
        .map(|(name, def)| ColumnSpec::new(name.clone(), schema_column_type(def)))
        .collect()
}

fn schema_column_type(def: &Value) -> ColumnType {
    scalar_column_type(def).unwrap_or(ColumnType::Text)
}

fn dedupe_columns(columns: Vec<ColumnSpec>, reserved: &[&str]) -> Vec<ColumnSpec> {
    let mut seen: HashSet<String> = reserved.iter().map(|name| (*name).to_string()).collect();
    let mut out = Vec::new();
    for col in columns {
        if seen.insert(col.name.clone()) {
            out.push(col);
        }
    }
    out
}

fn inferred_type(value: &Value) -> Option<ColumnType> {
    match value {
        Value::String(_) => Some(ColumnType::Text),
        Value::Number(_) => Some(ColumnType::Real),
        Value::Bool(_) => Some(ColumnType::Integer),
        _ => None,
    }
}

fn merge_column_types(left: ColumnType, right: ColumnType) -> ColumnType {
    if left == right {
        left
    } else {
        ColumnType::Text
    }
}

fn parse_stringified_json(value: &Value) -> Option<Value> {
    let Value::String(s) = value else {
        return None;
    };
    let parsed = serde_json::from_str::<Value>(s).ok()?;
    if parsed.is_object() {
        Some(parsed)
    } else {
        None
    }
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=? LIMIT 1",
        [table],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(DbError::from)
}

fn base_table(object: &str) -> String {
    format!("obj_{object}")
}

fn child_table(object: &str, prop: &str) -> String {
    format!("{}__{prop}", base_table(object))
}

fn index_ddl(table: &str, col: &str) -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS {} ON {}({});",
        qid(&format!("idx_{table}_{col}")),
        qid(table),
        qid(col)
    )
}

fn qid(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn json_path(prop: &str) -> String {
    if is_plain_json_key(prop) {
        format!("$.{prop}")
    } else {
        format!("$.\"{}\"", prop.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn is_plain_json_key(prop: &str) -> bool {
    let mut chars = prop.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    use rusqlite::params;
    use serde_json::json;

    fn synthetic_schema() -> Value {
        json!({
            "name": "alpha_user",
            "schema": {
                "properties": {
                    "active": { "type": "boolean", "searchable": true },
                    "age": { "type": "number" },
                    "displayName": { "type": "string" },
                    "kbaInfo": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "answer": { "type": "string" },
                                "customQuestion": { "type": "boolean" },
                                "questionId": { "type": "number" }
                            }
                        }
                    },
                    "loginHistory": {
                        "type": "array",
                        "items": { "type": "object" }
                    },
                    "manager": {
                        "type": "relationship",
                        "_refProperties": {
                            "properties": {
                                "kind": { "type": "string" }
                            }
                        }
                    },
                    "roles": {
                        "type": "array",
                        "items": {
                            "type": "relationship",
                            "_refProperties": {
                                "properties": {
                                    "assignmentType": { "type": "string" },
                                    "priority": { "type": "number" }
                                }
                            }
                        }
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "userName": { "type": "string", "searchable": true }
                }
            }
        })
    }

    fn login_history_columns() -> Vec<ColumnSpec> {
        vec![
            ColumnSpec::text("portal"),
            ColumnSpec::real("tdifLevel"),
            ColumnSpec::text("acr"),
            ColumnSpec::text("onBehalfOfOrg"),
            ColumnSpec::text("ts"),
        ]
    }

    fn store_with_login_history() -> Result<ObjectStore> {
        let mut overrides = ArrayColumnOverrides::new();
        overrides.insert("loginHistory".into(), login_history_columns());
        ObjectStore::new("alpha_user", &synthetic_schema(), &overrides)
    }

    #[test]
    fn ddl_generation_covers_generated_columns_and_child_shapes() -> Result<()> {
        let store = store_with_login_history()?;
        let ddl = store.ddl().join("\n");

        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS \"obj_alpha_user\""));
        assert!(
            ddl.contains("\"userName\" TEXT GENERATED ALWAYS AS (data->>'$.userName') VIRTUAL")
        );
        assert!(ddl.contains("\"active\" INTEGER GENERATED ALWAYS AS (data->>'$.active') VIRTUAL"));
        assert!(ddl.contains(
            "CREATE INDEX IF NOT EXISTS \"idx_obj_alpha_user_userName\" ON \"obj_alpha_user\"(\"userName\")"
        ));
        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS \"obj_alpha_user__tags\""));
        assert!(ddl.contains("\"value\" TEXT"));
        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS \"obj_alpha_user__roles\""));
        assert!(ddl.contains("\"ref_path\" TEXT"));
        assert!(ddl.contains("\"ref_id\" TEXT"));
        assert!(ddl.contains("\"assignmentType\" TEXT"));
        assert!(ddl.contains("\"priority\" REAL"));
        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS \"obj_alpha_user__manager\""));
        assert!(ddl.contains("\"kind\" TEXT"));
        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS \"obj_alpha_user__kbaInfo\""));
        assert!(ddl.contains("\"answer\" TEXT"));
        assert!(ddl.contains("\"customQuestion\" INTEGER"));
        assert!(ddl.contains("\"questionId\" REAL"));
        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS \"obj_alpha_user__loginHistory\""));
        assert!(ddl.contains("\"elem\" TEXT"));
        assert!(ddl.contains("\"portal\" TEXT"));
        assert!(ddl.contains("\"tdifLevel\" REAL"));
        assert!(
            ddl.contains("CREATE INDEX IF NOT EXISTS \"idx_obj_alpha_user__loginHistory_portal\"")
        );

        let conn = open(":memory:")?;
        create_schema(&conn, &store)?;
        Ok(())
    }

    #[test]
    fn infer_columns_samples_object_and_stringified_json_elements() {
        let columns = infer_columns(&[
            json!({
                "portal": "workforce",
                "tdifLevel": 2,
                "enabled": true,
                "ignored": { "nested": true }
            }),
            json!("{\"portal\":\"admin\",\"acr\":\"loa2\",\"tdifLevel\":3}"),
            json!({ "enabled": false, "onBehalfOfOrg": "org1" }),
        ]);

        assert_eq!(
            columns,
            vec![
                ColumnSpec::text("acr"),
                ColumnSpec::integer("enabled"),
                ColumnSpec::text("onBehalfOfOrg"),
                ColumnSpec::text("portal"),
                ColumnSpec::real("tdifLevel"),
            ]
        );
    }

    #[test]
    fn upsert_reconciles_child_rows() -> Result<()> {
        let mut conn = open(":memory:")?;
        let store = store_with_login_history()?;
        create_schema(&conn, &store)?;

        let first = json!({
            "_id": "user-1",
            "_rev": "rev-1",
            "_meta": {
                "_refResourceId": "meta-user-1",
                "lastChanged": { "date": "2026-06-20T00:00:00Z" }
            },
            "userName": "alice",
            "active": true,
            "tags": ["alpha", "beta"],
            "roles": [{
                "_ref": "managed/alpha_role/role-1",
                "_refProperties": { "assignmentType": "owner", "priority": 2 }
            }],
            "manager": {
                "_ref": "managed/alpha_user/manager-1",
                "_refProperties": { "kind": "direct" }
            },
            "kbaInfo": [
                { "answer": "a1", "customQuestion": false, "questionId": 10 },
                { "answer": "a2", "customQuestion": true, "questionId": 11 }
            ],
            "loginHistory": [
                { "portal": "workforce", "tdifLevel": 2, "acr": "loa2", "onBehalfOfOrg": "org", "ts": "2026-06-20T01:00:00Z" }
            ]
        });
        upsert_many(&mut conn, &store, [&first])?;

        let second = json!({
            "_id": "user-1",
            "_rev": "rev-2",
            "_meta": {
                "_refResourceId": "meta-user-1b",
                "lastChanged": { "date": "2026-06-21T00:00:00Z" }
            },
            "userName": "alice",
            "active": false,
            "tags": ["gamma"],
            "roles": [],
            "kbaInfo": [
                { "answer": "a3", "customQuestion": true, "questionId": 12 }
            ],
            "loginHistory": []
        });
        upsert_many(&mut conn, &store, [&second])?;

        let rev: String = conn.query_row(
            "SELECT rev FROM obj_alpha_user WHERE _id='user-1'",
            [],
            |row| row.get(0),
        )?;
        let meta_changed: String = conn.query_row(
            "SELECT meta_changed FROM obj_alpha_user WHERE _id='user-1'",
            [],
            |row| row.get(0),
        )?;
        let tag_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM obj_alpha_user__tags", [], |row| {
                row.get(0)
            })?;
        let tag: String = conn.query_row(
            "SELECT value FROM obj_alpha_user__tags WHERE parent_id='user-1'",
            [],
            |row| row.get(0),
        )?;
        let role_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM obj_alpha_user__roles", [], |row| {
                row.get(0)
            })?;
        let kba_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM obj_alpha_user__kbaInfo", [], |row| {
                row.get(0)
            })?;

        assert_eq!(rev, "rev-2");
        assert_eq!(meta_changed, "2026-06-21T00:00:00Z");
        assert_eq!(tag_count, 1);
        assert_eq!(tag, "gamma");
        assert_eq!(role_count, 0);
        assert_eq!(kba_count, 1);
        Ok(())
    }

    #[test]
    fn indexed_login_history_join_finds_matching_users() -> Result<()> {
        let inferred = infer_columns(&[json!({
            "portal": "workforce",
            "tdifLevel": 2,
            "acr": "loa2",
            "onBehalfOfOrg": "org",
            "ts": "2026-06-01T00:00:00Z"
        })]);
        let mut overrides = ArrayColumnOverrides::new();
        overrides.insert("loginHistory".into(), inferred);

        let mut conn = open(":memory:")?;
        let store = ObjectStore::new("alpha_user", &synthetic_schema(), &overrides)?;
        create_schema(&conn, &store)?;

        let users = vec![
            json!({
                "_id": "user-1",
                "userName": "alice",
                "loginHistory": [
                    { "portal": "workforce", "tdifLevel": 2, "acr": "loa2", "onBehalfOfOrg": "org-a", "ts": "2026-06-15T09:00:00Z" },
                    { "portal": "admin", "tdifLevel": 2, "acr": "loa2", "onBehalfOfOrg": "org-a", "ts": "2026-06-10T09:00:00Z" }
                ]
            }),
            json!({
                "_id": "user-2",
                "userName": "bob",
                "loginHistory": [
                    { "portal": "workforce", "tdifLevel": 2, "acr": "loa2", "onBehalfOfOrg": "org-b", "ts": "2026-05-20T09:00:00Z" },
                    "{\"portal\":\"workforce\",\"tdifLevel\":1,\"acr\":\"loa1\",\"onBehalfOfOrg\":\"org-b\",\"ts\":\"2026-06-18T09:00:00Z\"}"
                ]
            }),
        ];
        upsert_many(&mut conn, &store, &users)?;

        let mut stmt = conn.prepare(
            "SELECT DISTINCT u._id FROM obj_alpha_user__loginHistory h \
             JOIN obj_alpha_user u ON u._id=h.parent_id \
             WHERE h.portal=? AND h.tdifLevel=? AND h.ts >= ? \
             ORDER BY u._id",
        )?;
        let rows = stmt
            .query_map(
                params!["workforce", 2.0_f64, "2026-06-01T00:00:00Z"],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        assert_eq!(rows, vec!["user-1"]);
        Ok(())
    }

    #[test]
    fn sync_state_round_trips() -> Result<()> {
        let conn = open(":memory:")?;
        assert!(read_sync_state(&conn, "alpha_user")?.is_none());

        let first = SyncState {
            object: "alpha_user".into(),
            incremental_supported: true,
            watermark: Some("2026-06-20T00:00:00Z".into()),
            last_full_sync: Some("2026-06-20T01:00:00Z".into()),
        };
        write_sync_state(&conn, &first)?;
        assert_eq!(read_sync_state(&conn, "alpha_user")?, Some(first.clone()));

        let second = SyncState {
            watermark: Some("2026-06-21T00:00:00Z".into()),
            ..first
        };
        write_sync_state(&conn, &second)?;
        assert_eq!(list_sync_states(&conn)?, vec![second]);
        Ok(())
    }

    #[test]
    fn file_store_connections_apply_pragmas_and_write_disjoint_tables() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("aic-idmstore-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).expect("create temp IDM store dir");
        let path = dir.join("store.sqlite");

        let result = (|| -> Result<()> {
            let mut left = open(&path)?;
            let mut right = open(&path)?;

            let left_journal: String =
                left.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
            let right_timeout: i64 =
                right.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
            let right_foreign_keys: i64 =
                right.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
            assert_eq!(left_journal.to_ascii_lowercase(), "wal");
            assert_eq!(right_timeout, 30_000);
            assert_eq!(right_foreign_keys, 1);

            let overrides = ArrayColumnOverrides::new();
            let alpha = ObjectStore::new("alpha_user", &synthetic_schema(), &overrides)?;
            let bravo = ObjectStore::new("bravo_role", &synthetic_schema(), &overrides)?;
            create_schema(&left, &alpha)?;
            create_schema(&right, &bravo)?;

            upsert_many(
                &mut left,
                &alpha,
                [&json!({"_id": "user-1", "userName": "alice"})],
            )?;
            upsert_many(
                &mut right,
                &bravo,
                [&json!({"_id": "role-1", "displayName": "Role One"})],
            )?;

            assert_eq!(object_row_count(&left, "alpha_user")?, 1);
            assert_eq!(object_row_count(&right, "bravo_role")?, 1);
            Ok(())
        })();

        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    #[test]
    fn local_ids_meta_lookup_and_delete_helpers_use_base_table() -> Result<()> {
        let mut conn = open(":memory:")?;
        let store = store_with_login_history()?;
        create_schema(&conn, &store)?;

        let users = vec![
            json!({
                "_id": "user-1",
                "_meta": {
                    "_refResourceId": "meta-1",
                    "lastChanged": { "date": "2026-06-20T00:00:00Z" }
                },
                "tags": ["a", "b"]
            }),
            json!({
                "_id": "user-2",
                "_meta": {
                    "_id": "meta-2",
                    "lastChanged": { "date": "2026-06-21T00:00:00Z" }
                },
                "tags": ["c"]
            }),
        ];
        upsert_many(&mut conn, &store, &users)?;

        assert_eq!(
            local_ids(&conn, "alpha_user")?,
            BTreeSet::from(["user-1".to_string(), "user-2".to_string()])
        );
        let meta_ids = vec![
            "meta-1".to_string(),
            "meta-2".to_string(),
            "missing".to_string(),
        ];
        let mapped = record_ids_for_meta_ids(&conn, "alpha_user", &meta_ids)?;
        assert_eq!(mapped.get("meta-1").map(String::as_str), Some("user-1"));
        assert_eq!(mapped.get("meta-2").map(String::as_str), Some("user-2"));
        assert_eq!(mapped.get("missing"), None);

        let deleted = delete_records_by_id(&mut conn, "alpha_user", &["user-1".into()])?;
        assert_eq!(deleted, 1);
        assert_eq!(object_row_count(&conn, "alpha_user")?, 1);
        let tag_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM obj_alpha_user__tags", [], |row| {
                row.get(0)
            })?;
        assert_eq!(tag_count, 1);
        Ok(())
    }
}
