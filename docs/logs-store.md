# Local log store — sync, search, compact

Implemented in: `src/logs/`

This file documents pingone-aic-manager's **local** log store design: what `aic logs sync`,
`aic logs search`, and `aic logs compact` do on disk. The verified **remote**
API behaviour (endpoints, params, paging, rate limits, key-mint auth model,
source taxonomy, event payload shapes, journey join key) lives in
[`docs/api/08-logs.md`](api/08-logs.md) — read that first for anything that
touches the wire.

Why a local store at all: AIC retains logs for only **30 days** server-side.
Syncing locally gives offline history past that window, offline search, and a
compacted journey model that survives raw-event pruning.

## Store location & engine

- One **DuckDB** file per tenant: `.aic/logs/<tenant>.duckdb` (gitignored).
  Characters outside `[A-Za-z0-9._-]` in the tenant name are replaced with `_`
  (`src/logs/state.rs`).
- The engine is the **bundled DuckDB crate, version 1.4.5** (see `Cargo.lock`;
  `Cargo.toml` pins `duckdb = "1.4"` with the `bundled`, `json`, `chrono`
  features).

### ⚠ DuckDB version-skew hazard

DuckDB storage is not freely forward/backward compatible: a store file written
by a **newer** DuckDB can hang or fail an older engine (including this app's
bundled 1.4.5). Don't open the store with a newer external `duckdb` CLI / Python
client and then expect `aic` to read it — inspect with a matching version, or
work on a copy of the file.

## Schema

Ground truth is `init()` in `src/logs/db.rs`. Session pragmas:
`preserve_insertion_order = false`, `memory_limit = '2GB'`, `threads = 4`, and
the `json` extension.

### `log_events` — raw synced events

| Column           | Type      | Notes                                             |
| ---------------- | --------- | ------------------------------------------------- |
| `id`             | TEXT PK   | Payload `_id` if present, else a SHA-256 fallback |
| `ts`             | TIMESTAMP | Parsed from the event's RFC-3339 `timestamp`      |
| `source`         | TEXT      | e.g. `am-authentication`                          |
| `transaction_id` | TEXT      | Extracted from `payload.transactionId`            |
| `event_name`     | TEXT      | Extracted from `payload.eventName`                |
| `level`          | TEXT      | Extracted from `payload.level`                    |
| `topic`          | TEXT      | Extracted from `payload.topic`                    |
| `user_id`        | TEXT      | Extracted from `payload.userId`                   |
| `component`      | TEXT      | Extracted from `payload.component`                |
| `payload`        | JSON      | The full event payload (object or raw string)     |

Indexes on `ts`, `transaction_id`, `event_name`, `user_id`.

**Event identity & dedupe.** `id` is the payload's `_id` when it's a non-empty
string; otherwise `sha256(source|timestamp|payload_json)` hex. Inserts go
through a staging temp table, dedupe within the batch, then
`INSERT … ON CONFLICT (id) DO NOTHING` — so re-syncing an overlapping window
never duplicates rows.

### `sync_state` — per-source incremental cursor

| Column          | Type      | Notes                       |
| --------------- | --------- | --------------------------- |
| `source`        | TEXT PK   | One row per synced source   |
| `last_end_time` | TIMESTAMP | End of the last sync window |
| `updated_at`    | TIMESTAMP | Bookkeeping                 |

### Journey model (dimensions + fact)

Surrogate ids come from sequences (`journey_id_seq`, `node_id_seq`,
`outcome_id_seq`); dimension rows are get-or-create ("interned").

- `journey` — `id`, `name UNIQUE`.
- `node` — `id`, `journey_id`, `node_uuid`, `node_type`, `display_name`,
  `UNIQUE (journey_id, node_uuid)`. Descriptive columns refresh on repeat sight
  (last seen wins).
- `outcome` — `id`, `name UNIQUE`.
- `journey_attempt` — the fact table, one row per journey **execution**:

| Column             | Type                                            | Notes                                        |
| ------------------ | ----------------------------------------------- | -------------------------------------------- |
| `tracking_id`      | TEXT PK                                         | The full `trackingIds[0]` (see 08-logs)      |
| `journey_id`       | INTEGER                                         | → `journey.id`                               |
| `user_id`          | TEXT                                            | Tree event's `principal[0]`; NULL if no tree |
| `result`           | TEXT                                            | `COMPLETED` / `FAILED` / `ABANDONED`         |
| `furthest_node_id` | INTEGER                                         | Last node reached (→ `node.id`)              |
| `node_count`       | INTEGER                                         | Steps in `path`                              |
| `started_at`       | TIMESTAMP                                       | Min node ts, else tree ts                    |
| `ended_at`         | TIMESTAMP                                       | Tree ts, else max node ts                    |
| `path`             | `STRUCT(node_id INTEGER, outcome_id INTEGER)[]` | Ordered node steps with their outcomes       |

- `compact_state` — single row (`id`, `last_compacted`): the rollup watermark.

## `aic logs sync` — incremental sync

- **Curated default source list** (`DEFAULT_SYNC_SOURCES` in `src/logs/ops.rs`):

  ```
  am-authentication,am-access,am-activity,idm-activity,idm-config,idm-access
  ```

  Sync deliberately does **not** default to the `am-everything`/
  `idm-everything` rollups: in the sandbox sample those are ~99% `idm-core`
  raw-string FINE debug noise (no `_id`, no `eventName`, no user identity) — see
  the source-taxonomy section of `docs/api/08-logs.md`. The user-driven
  `tx`/`range`/`query` fetch commands keep `am-everything,idm-everything` as
  their defaults since they don't persist anything.

- **Cursor.** Per source, the next window starts at
  `sync_state.last_end_time − 5 min` (overlap re-fetches boundary events; the
  id-based dedupe absorbs them). A source with no cursor backfills **30 days**
  (the server-side retention limit). `--since <ISO-8601>` overrides the start
  for all requested sources.

- **`is_core_noise` filter** (`src/logs/ops.rs`), applied to every fetched page
  before insertion: an event is dropped iff its `source` ends with `-core`
  **and** its payload is a raw JSON _string_ that contains neither `WARN` nor
  `ERROR`. Structured (object) core payloads and WARN/ERROR raw strings are
  kept. Rationale: the `-core` streams are FINE-level Felix health-check /
  recon-queue / Quartz traces with zero audit signal. The filter **always runs
  during sync**, even when the user explicitly requests `--source idm-core` or
  `--source am-core`.

- Reports per source: `fetched` (from the API), `filtered` (dropped as core
  noise), `new` (inserted after dedupe).

## `aic logs search` — offline query

Reads the local DuckDB file only (errors with "run `aic logs sync` first" if the
store doesn't exist). Filters, all combined with AND, all bound as SQL
parameters (never interpolated):

| Flag                | Matches                                            |
| ------------------- | -------------------------------------------------- |
| `--tx <ID>`         | `transaction_id` equality                          |
| `--source <SOURCE>` | exact source id                                    |
| `--event <NAME>`    | `event_name` equality                              |
| `--user <ID>`       | `user_id` equality                                 |
| `--level <LEVEL>`   | `level` equality (INFO/WARN/ERROR)                 |
| `--begin <ISO>`     | `ts >= begin` (**inclusive**)                      |
| `--end <ISO>`       | `ts < end` (**exclusive**)                         |
| `--contains <TEXT>` | substring match over the payload text (`LIKE %…%`) |
| `--limit <N>`       | max rows (default **1000**)                        |
| `--count`           | print only the match count (ignores `--limit`)     |
| `--output <PATH>`   | write the JSON result to a file                    |

Results are reconstructed into the same API-shaped events that
`aic logs tx/range/query` print — `{"timestamp", "source", "payload"}` — ordered
by `ts`, so downstream `jq` pipelines work identically on live and local data.

## `aic logs compact` — journey rollup + retention

Offline: reads the existing store only; never touches the API or the vault.

1. **Rollup window.** Loads `am-authentication` payloads with
   `ts >= compact_state.last_compacted − 5 min` (everything, on first run).
2. **Grouping.** Groups events into executions by the **full
   `payload.trackingIds[0]`, verbatim** — the verified join key; see the
   join-key section of `docs/api/08-logs.md` for why `transactionId`, the
   stripped base UUID, and tree `_id` are all wrong.
   - `AM-NODE-LOGIN-COMPLETED` → one path step
     (`entries[0].info.{nodeId,nodeType,displayName,nodeOutcome}`), steps
     ordered by `payload.timestamp`.
   - `AM-TREE-LOGIN-COMPLETED` → the attempt's `result`, `user_id`
     (`principal[0]`), and authoritative journey name (`treeName`).
   - `AM-LOGIN-MODULE-COMPLETED` / `AM-LOGIN-COMPLETED` and anything else are
     non-journey auth (OAuth2 client / service-account module logins) — skipped.
3. **Result taxonomy.** Tree `result = SUCCESSFUL` → `COMPLETED`; any other tree
   `result` → `FAILED`; a node group with **no** tree event → `ABANDONED`.
4. **Interning + upsert.** Each distinct journey / node / outcome is interned
   exactly once per run, and all attempts are folded in with one set-based
   `INSERT … ON CONFLICT (tracking_id) DO UPDATE` (a per-step statement loop is
   prohibitively slow at DuckDB's per-statement planning cost). The upsert is
   idempotent across overlapping compact windows.
5. **Watermark + prune.** `compact_state.last_compacted` advances to now, then
   raw `log_events` older than `--retain-months` (default **3**) months are
   deleted. The rolled-up `journey_attempt` rows are kept forever — that's the
   compression: full journey history at a fraction of the raw-event size.

## See also

- [`docs/api/08-logs.md`](api/08-logs.md) — verified wire behaviour: log API
  endpoints/paging/rate limits, key-mint auth model (admin token only), source
  taxonomy, event shapes, and the journey join-key verification story.
- `docs/CLI.md` — the `aic logs` command reference.
