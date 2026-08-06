//! `aic idm` parser and command implementation.

use std::collections::BTreeMap;

use clap::Subcommand;
use rusqlite::Connection;
use rusqlite::types::Value as SqlValue;

use crate::cli::{print_json, print_table, tenant_for};
use crate::idmstore::state::{ObjectStatus, SyncReport};
use crate::idmstore::{db, ops, state};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum IdmCommand {
    /// Sync managed-object records into the local query store.
    Sync {
        /// Object names to sync, e.g. alpha_user. Omit for an interactive picker.
        objects: Vec<String>,
        #[arg(long, help = "Sync all syncable objects non-interactively")]
        all: bool,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Run a SQL query against the local query store.
    Query {
        sql: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// List syncable managed object names from the tenant.
    Objects {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Print object names as JSON")]
        json: bool,
    },
    /// List local query-store tables and columns.
    Tables {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Show local query-store status.
    Status {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
}

// ── clap parsing + dispatch ──────────────────────────────────────────────
pub async fn run(cmd: IdmCommand) -> Result<()> {
    match cmd {
        IdmCommand::Sync {
            objects,
            all,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let Some(objects) = resolve_sync_objects(&tenant, all, objects).await? else {
                return Ok(());
            };
            let report = ops::sync_tenant(&tenant, &objects).await?;
            print_sync_report(&report);
            Ok(())
        }
        IdmCommand::Query { sql, tenant } => {
            let tenant = tenant_for(tenant)?;
            let conn = open_existing_store(&tenant)?;
            println!("{}", run_query(&conn, &sql)?);
            Ok(())
        }
        IdmCommand::Objects { tenant, json } => {
            let tenant = tenant_for(tenant)?;
            let doc = crate::managed::api::get_managed(&tenant).await?;
            let names = ops::syncable_object_names(&doc)?;
            if json {
                print_json(&names)?;
            } else {
                let rows = names
                    .iter()
                    .map(|name| vec![name.clone()])
                    .collect::<Vec<_>>();
                print_table(&["OBJECT"], &rows);
            }
            Ok(())
        }
        IdmCommand::Tables { tenant } => {
            let tenant = tenant_for(tenant)?;
            let conn = open_existing_store(&tenant)?;
            println!("{}", render_tables(&introspect_tables(&conn)?));
            Ok(())
        }
        IdmCommand::Status { tenant } => {
            let tenant = tenant_for(tenant)?;
            let rows = ops::status(&tenant)?;
            print_status(&tenant, &rows);
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SyncRequest {
    All,
    Objects(Vec<String>),
    Pick,
}

fn resolve_sync_request(all: bool, objects: Vec<String>) -> SyncRequest {
    if all {
        SyncRequest::All
    } else if objects.is_empty() {
        SyncRequest::Pick
    } else {
        SyncRequest::Objects(objects)
    }
}

async fn resolve_sync_objects(
    tenant: &str,
    all: bool,
    objects: Vec<String>,
) -> Result<Option<Vec<String>>> {
    match resolve_sync_request(all, objects) {
        SyncRequest::All => Ok(Some(Vec::new())),
        SyncRequest::Objects(objects) => Ok(Some(objects)),
        SyncRequest::Pick => pick_sync_objects(tenant).await,
    }
}

// ── interactive picker ───────────────────────────────────────────────────
async fn pick_sync_objects(tenant: &str) -> Result<Option<Vec<String>>> {
    let doc = crate::managed::api::get_managed(tenant).await?;
    let mut candidates = ops::syncable_object_names(&doc)?;
    candidates.sort();
    prompt_sync_objects(candidates)
}

fn prompt_sync_objects(candidates: Vec<String>) -> Result<Option<Vec<String>>> {
    use inquire::{MultiSelect, error::InquireError};

    if candidates.is_empty() {
        println!("no syncable managed objects found");
        return Ok(None);
    }
    if crate::cli::prompting_disabled() {
        return Err(Error::Config(
            "object picker disabled by --no-prompt; pass object names explicitly or use `--all`"
                .into(),
        ));
    }

    match MultiSelect::new("Sync managed objects", candidates)
        .with_page_size(15)
        .raw_prompt()
    {
        Ok(selection) => {
            if selection.is_empty() {
                println!("nothing selected");
                return Ok(None);
            }
            Ok(Some(
                selection
                    .into_iter()
                    .map(|option| option.value)
                    .collect::<Vec<_>>(),
            ))
        }
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            println!("nothing selected");
            Ok(None)
        }
        Err(InquireError::NotTTY) => Err(Error::Config(
            "no terminal for the object picker; pass object names explicitly or use `--all`".into(),
        )),
        Err(error) => Err(Error::Config(format!("object picker: {error}"))),
    }
}

// ── read-only SQL runner + table renderer + schema introspection ─────────
fn open_existing_store(tenant: &str) -> Result<Connection> {
    let path = state::store_path(tenant);
    if !path.exists() {
        return Err(Error::Config(format!(
            "local IDM store for tenant '{tenant}' does not exist at {}; run `aic idm sync` first",
            path.display()
        )));
    }
    Ok(db::open_readonly(path)?)
}

fn run_query(conn: &Connection, sql: &str) -> Result<String> {
    let result = query_rows(conn, sql)?;
    Ok(render_table(&result.columns, &result.rows))
}

#[derive(Debug, Clone, PartialEq)]
struct QueryRows {
    columns: Vec<String>,
    rows: Vec<Vec<SqlValue>>,
}

fn query_rows(conn: &Connection, sql: &str) -> Result<QueryRows> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|error| sqlite_error("prepare IDM query", error))?;

    // The local store is opened read-only for CLI use, and sqlite3_stmt_readonly
    // rejects obvious non-query statements even in unit tests using writable DBs.
    if !stmt.readonly() {
        return Err(Error::Config(
            "idm query only supports read-only SQL; use SELECT or WITH against the local store"
                .into(),
        ));
    }
    if stmt.column_count() == 0 {
        return Err(Error::Config(
            "idm query must return columns; use SELECT or WITH".into(),
        ));
    }

    let columns = stmt
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let column_count = columns.len();
    let mut rows = stmt
        .query([])
        .map_err(|error| sqlite_error("execute IDM query", error))?;
    let mut out = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| sqlite_error("read IDM query row", error))?
    {
        let mut cells = Vec::with_capacity(column_count);
        for idx in 0..column_count {
            cells.push(
                row.get::<_, SqlValue>(idx)
                    .map_err(|error| sqlite_error("read IDM query value", error))?,
            );
        }
        out.push(cells);
    }

    Ok(QueryRows { columns, rows: out })
}

fn render_table(columns: &[String], rows: &[Vec<SqlValue>]) -> String {
    let rendered_rows = rows
        .iter()
        .map(|row| row.iter().map(render_cell).collect::<Vec<_>>())
        .collect::<Vec<_>>();

    let mut widths = columns
        .iter()
        .map(|column| column.chars().count())
        .collect::<Vec<_>>();
    for row in &rendered_rows {
        for (idx, cell) in row.iter().enumerate().take(widths.len()) {
            widths[idx] = widths[idx].max(cell.chars().count());
        }
    }

    let mut out = String::new();
    if !columns.is_empty() {
        out.push_str(&render_line(columns, &widths));
        out.push('\n');
        for row in &rendered_rows {
            out.push_str(&render_line(row, &widths));
            out.push('\n');
        }
    }
    out.push_str(&format!("({} rows)", rows.len()));
    out
}

fn render_cell(value: &SqlValue) -> String {
    match value {
        SqlValue::Null => "-".to_string(),
        SqlValue::Integer(value) => value.to_string(),
        SqlValue::Real(value) => value.to_string(),
        SqlValue::Text(value) => value.clone(),
        SqlValue::Blob(value) => format!("x'{}'", hex(value)),
    }
}

fn render_line(cells: &[String], widths: &[usize]) -> String {
    let mut out = String::new();
    for (idx, cell) in cells.iter().enumerate() {
        if idx > 0 {
            out.push_str("  ");
        }
        if idx + 1 == cells.len() {
            out.push_str(cell);
        } else {
            out.push_str(&format!("{cell:<width$}", width = widths[idx]));
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableInfo {
    name: String,
    columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableGroup {
    object: String,
    tables: Vec<TableInfo>,
}

fn introspect_tables(conn: &Connection) -> Result<Vec<TableGroup>> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type='table' AND name GLOB 'obj_*' \
             ORDER BY name",
        )
        .map_err(|error| sqlite_error("prepare IDM table list", error))?;
    let table_names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| sqlite_error("read IDM table list", error))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| sqlite_error("read IDM table name", error))?;

    let mut groups: BTreeMap<String, Vec<TableInfo>> = BTreeMap::new();
    for name in table_names {
        let object = table_object_name(&name);
        groups.entry(object).or_default().push(TableInfo {
            columns: table_columns(conn, &name)?,
            name,
        });
    }

    Ok(groups
        .into_iter()
        .map(|(object, tables)| TableGroup { object, tables })
        .collect())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_xinfo(?1) ORDER BY cid")
        .map_err(|error| sqlite_error("prepare IDM column list", error))?;
    stmt.query_map([table], |row| row.get::<_, String>(0))
        .map_err(|error| sqlite_error("read IDM column list", error))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| sqlite_error("read IDM column name", error))
}

fn table_object_name(table: &str) -> String {
    table
        .strip_prefix("obj_")
        .unwrap_or(table)
        .split_once("__")
        .map_or_else(
            || table.strip_prefix("obj_").unwrap_or(table).to_string(),
            |(object, _)| object.to_string(),
        )
}

fn render_tables(groups: &[TableGroup]) -> String {
    if groups.is_empty() {
        return "(no local IDM object tables)".to_string();
    }

    let mut out = String::new();
    for group in groups {
        out.push_str(&group.object);
        out.push('\n');
        for table in &group.tables {
            out.push_str("  ");
            out.push_str(&table.name);
            out.push_str(": ");
            out.push_str(&table.columns.join(", "));
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

fn sqlite_error(action: &str, error: rusqlite::Error) -> Error {
    Error::Config(format!("{action}: {error}"))
}

fn print_sync_report(report: &SyncReport) {
    println!("tenant: {}", report.tenant);
    println!("store:  {}", report.store_path.display());
    if report.objects.is_empty() {
        println!("(no syncable managed objects)");
        return;
    }
    for object in &report.objects {
        println!(
            "{:<28} {:<11} rows={:<8} fetched={:<8} upserted={:<8} deleted={:<6} incremental={} watermark={} last_full={}",
            object.object,
            object.mode.label(),
            object.rows,
            object.fetched,
            object.upserted,
            object.deleted,
            yes_no(object.incremental_supported),
            opt(&object.watermark),
            opt(&object.last_full_sync),
        );
    }
}

fn print_status(tenant: &str, rows: &[ObjectStatus]) {
    println!("tenant: {tenant}");
    if rows.is_empty() {
        println!("(no local IDM sync state)");
        return;
    }
    for row in rows {
        println!(
            "{:<28} rows={:<8} incremental={} watermark={} last_full={}",
            row.object,
            row.rows,
            yes_no(row.incremental_supported),
            opt(&row.watermark),
            opt(&row.last_full_sync),
        );
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn opt(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_sync_request_prefers_all() {
        assert_eq!(
            resolve_sync_request(true, vec!["alpha_user".into()]),
            SyncRequest::All
        );
    }

    #[test]
    fn resolve_sync_request_uses_explicit_objects() {
        assert_eq!(
            resolve_sync_request(false, vec!["alpha_user".into(), "bravo_user".into()]),
            SyncRequest::Objects(vec!["alpha_user".into(), "bravo_user".into()])
        );
    }

    #[test]
    fn resolve_sync_request_picks_when_empty() {
        assert_eq!(resolve_sync_request(false, Vec::new()), SyncRequest::Pick);
    }

    #[test]
    fn render_table_aligns_values_and_marks_nulls() {
        let columns = vec!["name".to_string(), "score".to_string(), "note".to_string()];
        let rows = vec![
            vec![
                SqlValue::Text("alice".into()),
                SqlValue::Integer(7),
                SqlValue::Null,
            ],
            vec![
                SqlValue::Text("bob".into()),
                SqlValue::Real(12.5),
                SqlValue::Text("ready".into()),
            ],
        ];

        assert_eq!(
            render_table(&columns, &rows),
            "name   score  note\nalice  7      -\nbob    12.5   ready\n(2 rows)"
        );
    }

    #[test]
    fn query_connection_runs_join_and_renders_rows() -> Result<()> {
        let conn =
            Connection::open_in_memory().map_err(|error| sqlite_error("open test DB", error))?;
        conn.execute_batch(
            "CREATE TABLE obj_x (_id TEXT PRIMARY KEY, userName TEXT);
             CREATE TABLE obj_x__logins (parent_id TEXT, idx INTEGER, portal TEXT);
             INSERT INTO obj_x VALUES ('u1', 'alice'), ('u2', 'bob');
             INSERT INTO obj_x__logins VALUES ('u1', 0, 'workforce'), ('u2', 0, 'admin');",
        )
        .map_err(|error| sqlite_error("seed test DB", error))?;

        let rendered = run_query(
            &conn,
            "SELECT u._id, u.userName, l.portal \
             FROM obj_x u JOIN obj_x__logins l ON l.parent_id = u._id \
             WHERE l.portal = 'workforce'",
        )?;

        assert_eq!(
            rendered,
            "_id  userName  portal\nu1   alice     workforce\n(1 rows)"
        );
        Ok(())
    }

    #[test]
    fn query_connection_rejects_writes() -> Result<()> {
        let conn =
            Connection::open_in_memory().map_err(|error| sqlite_error("open test DB", error))?;
        conn.execute_batch("CREATE TABLE obj_x (_id TEXT PRIMARY KEY);")
            .map_err(|error| sqlite_error("seed test DB", error))?;

        let error = run_query(&conn, "DELETE FROM obj_x").unwrap_err();

        assert!(error.to_string().contains("read-only SQL"));
        Ok(())
    }

    #[test]
    fn introspect_tables_groups_object_and_child_tables() -> Result<()> {
        let conn =
            Connection::open_in_memory().map_err(|error| sqlite_error("open test DB", error))?;
        conn.execute_batch(
            "CREATE TABLE sync_state (object TEXT PRIMARY KEY);
             CREATE TABLE obj_alpha_user (_id TEXT PRIMARY KEY, data TEXT, userName TEXT);
             CREATE TABLE obj_alpha_user__roles (parent_id TEXT, idx INTEGER, ref_id TEXT);
             CREATE TABLE obj_bravo_role (_id TEXT PRIMARY KEY, name TEXT);",
        )
        .map_err(|error| sqlite_error("seed test DB", error))?;

        let groups = introspect_tables(&conn)?;

        assert_eq!(
            groups,
            vec![
                TableGroup {
                    object: "alpha_user".into(),
                    tables: vec![
                        TableInfo {
                            name: "obj_alpha_user".into(),
                            columns: vec!["_id".into(), "data".into(), "userName".into()],
                        },
                        TableInfo {
                            name: "obj_alpha_user__roles".into(),
                            columns: vec!["parent_id".into(), "idx".into(), "ref_id".into()],
                        },
                    ],
                },
                TableGroup {
                    object: "bravo_role".into(),
                    tables: vec![TableInfo {
                        name: "obj_bravo_role".into(),
                        columns: vec!["_id".into(), "name".into()],
                    }],
                },
            ]
        );
        assert_eq!(
            render_tables(&groups),
            "alpha_user\n  obj_alpha_user: _id, data, userName\n  obj_alpha_user__roles: parent_id, idx, ref_id\nbravo_role\n  obj_bravo_role: _id, name"
        );
        Ok(())
    }
}
