# aic CLI reference

`aic` is a single binary. With **no arguments it launches the TUI**; with a
subcommand it runs as a `kubectl`-style CLI. Both surfaces talk to your tenant
through the same background **agent** (see [The agent](../README.md#the-agent)),
so you authenticate once and every command reuses the unlocked session.

This is the complete command reference. Every command also has built-in help:

```bash
aic --help
aic <command> --help
aic <command> <subcommand> --help
```

---

## Conventions that apply everywhere

- **Project-rooted.** `aic` walks up from your current directory to find the
  project root (the directory containing `.aic-edit/`), so any command works
  from any subdirectory.
- **`--tenant <name>`** overrides the active context for a single call. With no
  flag, commands use the current context (`aic ctx current`); the default
  context name is `sandbox`. For `script` commands the tenant is also inferred
  from a `workspace/<tenant>/` path you're inside.
- **`--realm <alpha|bravo>`** selects the AM realm for realm-scoped commands
  (`journey`, `oauth`, `secretmap`, and AM `script` namespaces). Defaults to
  **`alpha`**. ESVs and IDM endpoints are tenant-global and take no realm.
- **Production-write guard.** Commands that mutate a *production-themed* tenant
  refuse to run without **`--yes`** — the CLI equivalent of the TUI's prod
  guard. Irreversible commands (`esv secret destroy`/`delete`) prompt for a
  typed confirmation on *any* tenant unless `--yes` is given.
- **`--force`** skips a safety check specific to the command (e.g. overwriting a
  drifted remote, deleting a journey/client). It's called out per command below.
- **JSON output.** Read commands generally print raw JSON (the tenant's
  `result` shape) so you can pipe to `jq`. Some commands take `--json` for a
  machine-readable form of an otherwise human-formatted listing.

---

## Session & agent

The agent holds your decrypted service-account key in memory and mints/refreshes
bearer tokens. See the [agent section of the README](../README.md#the-agent) for
the locked/unlocked model and why `logout` ≠ `stop`.

| Command | What it does |
|---|---|
| `aic agent` | Run the agent in the foreground (Ctrl-C to stop; logs to stderr). Normally auto-spawned — you rarely run this directly. |
| `aic agent --detach` | Spawn a detached agent (logs to `.aic-edit/agent.log`) and exit. |
| `aic agent --idle-timeout <seconds>` | Override the auto-lock timeout (default 3600s, or `settings.toml`). |
| `aic login` | Unlock the agent (no-echo master-password prompt). |
| `aic logout` | **Lock** the agent — wipe keys + tokens from memory, leave it running. |
| `aic stop` | **Stop** the agent process entirely. |
| `aic status` | Show whether the agent is running/unlocked, the active tenant, and token expiry. |

### Context

| Command | What it does |
|---|---|
| `aic ctx list` | List tenants defined in `.aic-edit/config.toml`. |
| `aic ctx current` | Print the active context. |
| `aic ctx use <tenant>` | Switch the active context. |
| `aic whoami [--tenant <name>]` | Mint and print token info for a context. |
| `aic whoami --token` | Print **only** the bearer token (for scripting, e.g. `curl -H "Authorization: Bearer $(aic whoami --token)"`). |

---

## `aic esv` — environment variables & secrets

ESVs are tenant-global. Changes to variables/secrets are staged on the tenant
and only take effect after a runtime restart (`aic esv apply`).

### Variables

```bash
aic esv list                                    # all variables (JSON result array)
aic esv get esv-my-var                           # one variable as JSON
aic esv set esv-my-var --value hello --type string [--description "…"] [--yes]
aic esv delete esv-my-var [--yes]
aic esv apply [--yes]                            # restart the runtime to apply staged changes
```

`--type` (`expressionType`) is one of `string`, `int`, `bool`, `list`, `object`,
`array`, `keyvaluelist` (default `string`). Values are stored base64-encoded.

> Restarts are rate-limited more tightly than reads — don't `apply` in a loop.

### Secrets (versioned, write-only)

Secret *values* are never readable back; commands return metadata only.

```bash
aic esv secret list                              # metadata for all secrets
aic esv secret get esv-my-secret                 # one secret's metadata
aic esv secret create esv-my-secret              # create (prompts, no echo)
aic esv secret versions esv-my-secret            # versions, newest first
aic esv secret add-version esv-my-secret         # add + activate a new version
aic esv secret enable  esv-my-secret 2
aic esv secret disable esv-my-secret 2           # latest version can't be disabled
aic esv secret set-description esv-my-secret --description "…"
aic esv secret destroy esv-my-secret 2 --yes     # irreversible — destroy one version
aic esv secret delete  esv-my-secret --yes       # irreversible — delete the secret
```

**Value sources** (for `create` / `add-version`), in priority order:

1. `--value-file <path>` — read from a file (one trailing newline stripped).
2. `--value-stdin` — read from stdin (e.g. `printf 'secret' | aic esv secret add-version … --value-stdin`).
3. interactive no-echo prompt (default if none given).

`--value <v>` exists for scripting but is **discouraged** — it leaks into shell
history and `ps`. `create` is create-only (PUT); change a value with
`add-version`, which becomes the active version.

---

## `aic logs` — fetch, sync, search, compact

Logs use the tenant's separate API-key auth plane. `key create` mints a key
pair only while an admin-user session is available; the service-account bearer
cannot mint or read log keys.

### Key management

| Command | What it does |
|---|---|
| `aic logs key set [--tenant <name>] [--id <api_key_id>]` | Store or replace the log API key pair in the vault. Prompts for the secret. |
| `aic logs key show [--tenant <name>]` | Show whether a log API key pair is stored, and print the key id. |
| `aic logs key rm [--tenant <name>]` | Remove the stored log API key pair. |
| `aic logs key create [--tenant <name>] [--cookie-name <name>]` | Mint a new key pair from an admin session, then store it. Prompts for the AM session cookie value if needed. |

### Remote fetch

| Command | What it does |
|---|---|
| `aic logs sources [--tenant <name>] [--json] [--output <path>]` | List available log source ids. `--json` prints the list as JSON; `--output` writes it to a file. |
| `aic logs tx <transaction_id> [--tenant <name>] [--source <csv>] [--output <path>]` | Fetch all events for one transaction id. `--source` narrows to a comma-separated source list. |
| `aic logs range <begin> <end> [--tenant <name>] [--source <csv>] [--query <crest>] [--output <path>]` | Fetch events in an ISO-8601 time range. `--query` adds an optional CREST filter. |
| `aic logs query <filter> [--begin <iso>] [--end <iso>] [--tenant <name>] [--source <csv>] [--output <path>]` | Run a CREST filter over the logs API. Defaults to the most recent 24 hours. |

### Local store

| Command | What it does |
|---|---|
| `aic logs search [--tenant <name>] [--tx <id>] [--source <source>] [--event <name>] [--user <id>] [--level <level>] [--begin <iso>] [--end <iso>] [--contains <text>] [--limit <n>] [--count] [--output <path>]` | Query the synced DuckDB store offline. `--count` prints only the match count; `--output` writes JSON results. |
| `aic logs sync [--tenant <name>] [--source <csv>] [--since <iso>]` | Incrementally sync log sources into the local DuckDB store. Defaults to the curated source list when `--source` is omitted. |
| `aic logs compact [--tenant <name>] [--retain-months <n>]` | Roll up journeys from `am-authentication` and prune raw events older than the retention window. Default retention is 3 months. |

---

## `aic managed` — IDM managed-object schema

Inspects the per-tenant IDM managed-object **schema** (not the records — for
record data use `aic idm`). Object hooks (`onCreate`/`onUpdate`/…) sync as
workspace scripts via `aic script` (`managed/<object>.<hook>`).

```bash
aic managed list                                 # object types with property + hook counts
aic managed get alpha_user                        # one object's full definition as JSON
```

---

## `aic idm` — local record store & query

Syncs IDM managed-object **records** into a local SQLite store
(`.aic-edit/idmstore/<tenant>.sqlite`, gitignored) so you can query them with
SQL — including joins into nested arrays. Each object becomes a base table
`obj_<type>` (full record JSON in `data`, plus generated columns for top-level
scalar fields), with child tables `obj_<type>__<field>` for arrays and
relationships.

```bash
aic idm objects                                  # list syncable object names (live, from the tenant)
aic idm sync                                     # interactive multiselect → sync chosen objects
aic idm sync alpha_user bravo_user                # sync named objects
aic idm sync --all                               # sync every syncable object, non-interactively
aic idm status                                   # per-object: rows, incremental flag, watermark
aic idm tables                                   # local tables + columns (discoverability)
aic idm query "SELECT userName, accountStatus FROM obj_alpha_user WHERE accountStatus='active'"
```

**Incremental sync.** User objects (`alpha_user`/`bravo_user`) re-sync only
records changed since the last run (via IDM's `_meta` change timestamp) plus an
id-diff for creates/deletes. Other objects (no per-record change signal) are
re-pulled in full. Re-running `sync` after a period brings the store up to date.

**Querying nested arrays.** A login-history style array shredded into a child
table is a clean indexed join:

```bash
aic idm query "
  SELECT DISTINCT u.userName
  FROM obj_alpha_user__loginHistory h
  JOIN obj_alpha_user u ON u._id = h.parent_id
  WHERE h.portal = 'mygov' AND h.tdifLevel = 'IP3'
    AND h.ts >= datetime('now','-7 days')"
```

`query` is **read-only** (writes are rejected). Run `aic idm tables` to discover
table and column names.

---

## `aic journey` — authentication trees

Realm-scoped. Journeys pull/push as JSON exports (tree + all its nodes) under
the workspace.

```bash
aic journey list [--realm alpha]                       # journey names
aic journey pull <name> [--realm alpha]                # tree + nodes → workspace JSON
aic journey push <name> [--realm alpha] [--force]      # push an export back
aic journey delete <name> --force [--realm alpha]      # delete (requires --force)
aic journey using-script <script-uuid> [--realm alpha] # journeys referencing a script
aic journey nodes [--realm alpha]                      # available node types
aic journey node-schema <nodeType> [--realm alpha]     # a node type's schema (JSON)
aic journey node-template <nodeType> [--realm alpha]   # a starter node config (JSON)
```

---

## `aic oauth` — OAuth2 clients

Realm-scoped. Clients pull/push as JSON under the workspace.

```bash
aic oauth list [--realm alpha]                    # client ids
aic oauth pull <id> [--realm alpha]               # one client → workspace JSON
aic oauth push <id> [--realm alpha] [--force]     # push a workspace client JSON back
aic oauth delete <id> --force [--realm alpha]     # delete (requires --force)
```

> `*-encrypted` fields are cluster-local and stripped on push; `_rev` is ignored
> (plain PUT). See `docs/api/05-oauth2-oidc.md`.

---

## `aic secretmap` — AM secret-label → ESV-secret mappings

Realm-scoped. Re-point AM secret *labels* (purposes) at existing ESV secrets.

```bash
aic secretmap list [--realm alpha] [--json]            # configured mappings
aic secretmap list-labels [--realm alpha]              # valid AM secret labels (alias: labels)
aic secretmap get <secret-label> [--realm alpha]       # one raw mapping
aic secretmap set <secret-label> <esv-secret-id> [--realm alpha] [--force]
aic secretmap remove <secret-label> [--realm alpha]    # alias: delete
```

---

## `aic script` — typed script workspace sync

Two-way sync of AIC scripts to a local **typed workspace** at
`./workspace/<tenant>/` (one tree per tenant) with `.d.ts` definitions +
ESLint/TypeScript config, so your editor gets full IntelliSense on script
bodies. Four script "kinds" sit behind one engine:

- **AM scripts** — realm-scoped, under `am/<realm>/<type>/` (e.g.
  `decision-node`, `lib`, `oidc-claims`; Groovy scripts aren't synced).
- **IDM custom endpoints** — tenant-global under `idm/endpoint/`.
- **IDM scheduled jobs** — tenant-global under `idm/schedule/` (script-invoking
  schedules only).
- **IDM managed-object hooks** — under `idm/managed/<object>/<hook>.cjs`
  (file-backed hooks are read-only).

### The `<ref>` model

Scripts are addressed by a **full-name** `<namespace>/<name>`, where the
namespace is `alpha`/`bravo` (AM realm), `endpoint`, `schedule`, `sync`, or `managed`
(hook name is `<object>.<hook>`, e.g. `managed/alpha_user.onCreate`). So you
never pass `--kind`/`--realm` to script commands. A bare `<name>` resolves its
namespace from your current directory. A bare namespace (`bravo`, `endpoint`)
means "all of it"; `all` means everything.

### Commands

```bash
aic script workspace init                       # scaffold the tenant tree (both realms + idm)
aic script workspace update                     # refresh bundled types/config to the latest
aic script list [<ref>]                         # list scripts (each row tagged with its `ref`)
aic script pull [<ref>] [--force]               # pull; no ref → fuzzy picker
aic script push [<ref>] [--force] [--yes]       # push local edits; no ref → fuzzy picker
aic script sync [<ref>] [--resolve local|remote] # reconcile: push local-only, pull remote-only
aic script watch                                # auto-push each .cjs you save (Ctrl-C to stop)
aic script status [<ref>]                       # in sync / modified / remote / conflict
aic script diff [<ref>] [--local-vs-snapshot | --snapshot-vs-remote]
```

- **Fuzzy picker.** `pull`/`push` with no `<ref>` open an interactive picker
  (type to filter). Lines are marked `!` (local changes) or `-` (not pulled);
  on `push`, locally-changed scripts sort first.
- **Conflict detection is content-based** (scripts have no `_rev`): a push only
  proceeds if the remote still matches what you last synced — even if the
  revision moved but the content reverted. If the remote content drifted, the
  push is blocked and a 3-way diff is shown; `--force` overrides.
- **`status` filters.** `am`/`idm` are group aliases; anything else is a
  case-insensitive substring of the full-name (use a trailing slash, e.g.
  `alpha/`, to match only that AM realm and exclude `managed/alpha_user…`).
- **`diff`** shells out to `git diff --no-index` (needs `git` on PATH): colored
  via your pager interactively, plain unified diff when piped
  (`aic script diff bravo/Foo | delta`). Default compares local vs tenant;
  `--local-vs-snapshot` shows your edits since the last pull,
  `--snapshot-vs-remote` shows tenant drift since you pulled.

> After upgrading the binary, restart the agent (`aic stop` then `aic login`)
> so it loads new `Accept-API-Version` headers used by AM-script support.

---

## See also

- [README](../README.md) — what the tool is, setup, and the agent model.
- [`docs/api/`](api/) — verified AIC endpoint reference (read before changing
  code that hits a tenant).
