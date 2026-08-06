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
  project root (the directory containing `.aic/`), so any command works from any
  subdirectory.
- **`--tenant <name>`** overrides the active context for a single call. With no
  flag, commands use the current context (`aic ctx current`); the default
  context name is `sandbox`. For `script` commands the tenant is also inferred
  from a `workspace/<tenant>/` path you're inside.
- **`--realm <alpha|bravo>`** selects the AM realm for realm-scoped commands
  (`journey`, `jwt-bearer`, `oauth`, `secretmap`, and AM `script` namespaces).
  Defaults to **`alpha`**. ESVs and IDM endpoints are tenant-global and take no
  realm.
- **Production-write guard.** Commands that mutate a _production-themed_ tenant
  refuse to run without **`--yes`** — the CLI equivalent of the TUI's prod
  guard. Irreversible commands (`esv secret destroy`/`delete`) prompt for a
  typed confirmation on _any_ tenant unless `--yes` is given.
- **`--force`** skips a safety check specific to the command (e.g. overwriting a
  drifted remote, deleting a journey/client). It's called out per command below.
- **Output format.** List commands default to kubectl-style tables. Pass
  `--json` on list commands for machine-readable output. Single-resource reads
  and export-style commands still print JSON by default.
- **Non-interactive mode.** Pass the global `--no-prompt` flag, or set
  `AIC_NO_PROMPT=1`, to disable every interactive prompt. If input is required,
  the command fails instead of waiting on a terminal. Confirming a missing
  operator name is optional: non-interactive commands use the best fallback for
  that run and leave the setting unset for a later real terminal.

### Exit codes

| Code | Meaning                                                                 |
| ---- | ----------------------------------------------------------------------- |
| `0`  | Success                                                                 |
| `1`  | General error, including invalid credentials                            |
| `3`  | The agent is locked and the command could not prompt for authentication |

---

## Session & agent

The agent holds your decrypted service-account key in memory and mints/refreshes
bearer tokens. See the [agent section of the README](../README.md#the-agent) for
the locked/unlocked model and why `logout` ≠ `stop`.

| Command                              | What it does                                                                                                            |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| `aic agent`                          | Run the agent in the foreground (Ctrl-C to stop; logs to stderr). Normally auto-spawned — you rarely run this directly. |
| `aic agent --detach`                 | Spawn a detached agent (logs to `.aic/agent.log`) and exit.                                                             |
| `aic agent --idle-timeout <seconds>` | Override the auto-lock timeout (default 3600s, or `settings.toml`).                                                     |
| `aic session login`                  | Unlock the agent (no-echo master-password prompt).                                                                      |
| `aic session login --password-stdin` | Read one master-password line from stdin and unlock without prompting.                                                  |
| `aic session logout`                 | **Lock** the agent — wipe keys + tokens from memory, leave it running.                                                  |
| `aic session stop`                   | **Stop** the agent process entirely.                                                                                    |
| `aic session status`                 | Show whether the agent is running/unlocked, the active tenant, and token expiry.                                        |

The older top-level `aic login`, `aic logout`, `aic stop`, and `aic status`
forms still work as compatibility aliases, but are hidden from help.

Tenant commands pre-flight the agent session. A locked command prompts only when
stdin and stderr are terminals and `/dev/tty` is available; the prompt times out
after 60 seconds. For automation, either unlock explicitly or pass `--no-prompt`
so a locked session exits with status 3. To unlock without a terminal, pipe
exactly one password line:

```sh
printf '%s\n' "$PASSWORD" | aic session login --password-stdin --no-prompt
aic --no-prompt esv list
```

`--password-stdin` selects the password factor when both a password and a
security key are enrolled. It fails if there is no enrolled password factor. The
binary deliberately does not read passwords from environment variables.

### Context

| Command                        | What it does                                                                                                   |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| `aic ctx list [--json]`        | List tenants defined in `.aic/config.toml`.                                                                    |
| `aic ctx current`              | Print the active context.                                                                                      |
| `aic ctx use <tenant>`         | Switch the active context.                                                                                     |
| `aic whoami [--tenant <name>]` | Mint and print token info plus the local operator name and host for a context.                                 |
| `aic whoami --token`           | Print **only** the bearer token (for scripting, e.g. `curl -H "Authorization: Bearer $(aic whoami --token)"`). |

The normal `whoami` output includes `operator: <name> on <host>`. When the name
has not been saved yet, the line says it is unset and points to
`aic settings set operator.name <name>`. `--token` remains exactly one bare
token on stdout.

### aic auth — mint a token as an end user

    aic auth --as-id <uuid> --client-id <id> [--client-secret-stdin] [--scope S]...
    aic auth --as-username <name> --client-id <id> [--scope S]...
    aic auth ... --token

Exactly one of --as-id and --as-username is required. Usernames are resolved
to their IDM managed-object UUID before signing. The client secret is read
from stdin when --client-secret-stdin is supplied; otherwise an interactive
command prompts on the terminal. Secrets are never accepted as argv or
environment values. The command refuses production-themed tenants and
requires a key from aic jwt-bearer setup.

Default output includes the user, client, granted scope, expiry, signing kid,
and a redacted token. --token prints only the bare access token.

### Settings

```bash
aic settings list
aic settings get operator.name
aic settings set operator.name dsbalmain@agiledigital.com.au
aic settings set operator.host daves-laptop
aic settings set agent-idle-timeout-secs 3600
```

`list` shows every supported key's effective value and whether it is defaulted.
Operator defaults are derived locally for this command; `aic settings` does not
unlock the agent or contact a tenant. Supported keys are `operator.name`,
`operator.host`, and `agent-idle-timeout-secs`.

`version` is managed by `aic`. `encrypt_keys` is deliberately not settable here:
changing vault encryption requires the TUI's **Auth Settings** transition so the
`.enc`/`.plain` files and the flag cannot get out of sync.

---

## `aic esv` — environment variables & secrets

ESVs are tenant-global. Changes to variables/secrets are staged on the tenant
and only take effect after a runtime restart (`aic esv apply`).

### Variables

```bash
aic esv list [--json]                           # all variables (table by default)
aic esv get esv-my-var                           # one variable as JSON
aic esv set esv-my-var --value hello --type string [--description "…"] [--yes]
aic esv delete esv-my-var [--yes]
aic esv apply [--yes]                            # restart the runtime to apply staged changes
```

`--type` (`expressionType`) is one of `string`, `int`, `bool`, `list`, `object`,
`array`, `keyvaluelist` (default `string`). Values are stored base64-encoded.

> Restarts are rate-limited more tightly than reads — don't `apply` in a loop.

### Secrets (versioned, write-only)

Secret _values_ are never readable back; commands return metadata only.

```bash
aic esv secret list [--json]                     # metadata for all secrets
aic esv secret get esv-my-secret                 # one secret's metadata
aic esv secret create esv-my-secret              # create (prompts, no echo)
aic esv secret versions esv-my-secret [--json]   # versions, newest first
aic esv secret add-version esv-my-secret         # add + activate a new version
aic esv secret enable  esv-my-secret 2
aic esv secret disable esv-my-secret 2           # latest version can't be disabled
aic esv secret set-description esv-my-secret --description "…"
aic esv secret destroy esv-my-secret 2 --yes     # irreversible — destroy one version
aic esv secret delete  esv-my-secret --yes       # irreversible — delete the secret
```

**Value sources** (for `create` / `add-version`), in priority order:

1. `--value-file <path>` — read from a file (one trailing newline stripped).
2. `--value-stdin` — read from stdin (e.g.
   `printf 'secret' | aic esv secret add-version … --value-stdin`).
3. interactive no-echo prompt (default if none given).

`--value <v>` exists for scripting but is **discouraged** — it leaks into shell
history and `ps`. `create` is create-only (PUT); change a value with
`add-version`, which becomes the active version.

---

## `aic logs` — fetch, sync, search, compact

Logs use the tenant's separate API-key auth plane. `key create` mints a key pair
only while an admin-user session is available; the service-account bearer cannot
mint or read log keys. The resolved admin username names the remote credential,
but this standalone command does not set `operator.name`.

### Key management

| Command                                                        | What it does                                                                                                 |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `aic logs key set [--tenant <name>] [--id <api_key_id>]`       | Store or replace the log API key pair in the vault. Prompts for the secret.                                  |
| `aic logs key show [--tenant <name>]`                          | Show whether a log API key pair is stored, and print the key id.                                             |
| `aic logs key rm [--tenant <name>]`                            | Remove the stored log API key pair.                                                                          |
| `aic logs key create [--tenant <name>] [--cookie-name <name>]` | Mint a new key pair from an admin session, then store it. Prompts for the AM session cookie value if needed. |

### Remote fetch

| Command                                                                                                      | What it does                                                                                     |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `aic logs sources [--tenant <name>] [--json] [--output <path>]`                                              | List available log source ids. `--json` prints the list as JSON; `--output` writes it to a file. |
| `aic logs tx <transaction_id> [--tenant <name>] [--source <csv>] [--output <path>]`                          | Fetch all events for one transaction id. `--source` narrows to a comma-separated source list.    |
| `aic logs range <begin> <end> [--tenant <name>] [--source <csv>] [--query <crest>] [--output <path>]`        | Fetch events in an ISO-8601 time range. `--query` adds an optional CREST filter.                 |
| `aic logs query <filter> [--begin <iso>] [--end <iso>] [--tenant <name>] [--source <csv>] [--output <path>]` | Run a CREST filter over the logs API. Defaults to the most recent 24 hours.                      |

### Local store

| Command                                                                                                                                                                                                          | What it does                                                                                                                   |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `aic logs search [--tenant <name>] [--tx <id>] [--source <source>] [--event <name>] [--user <id>] [--level <level>] [--begin <iso>] [--end <iso>] [--contains <text>] [--limit <n>] [--count] [--output <path>]` | Query the synced DuckDB store offline. `--count` prints only the match count; `--output` writes JSON results.                  |
| `aic logs sync [--tenant <name>] [--source <csv>] [--since <iso>]`                                                                                                                                               | Incrementally sync log sources into the local DuckDB store. Defaults to the curated source list when `--source` is omitted.    |
| `aic logs compact [--tenant <name>] [--retain-months <n>]`                                                                                                                                                       | Roll up journeys from `am-authentication` and prune raw events older than the retention window. Default retention is 3 months. |

---

## `aic managed` — IDM managed-object schema

Inspects and edits the per-tenant IDM managed-object **schema** (not the records
— for record data use `aic idm`). Object hooks (`onCreate`/`onUpdate`/ …) sync
as workspace scripts via `aic script` (`managed/<object>.<hook>`).

```bash
aic managed list [--json]                        # object types with property + hook counts
aic managed get alpha_user                        # one object's full definition as JSON
aic managed object create custom_widget [--title T] [--description D] [--yes] [--json]
aic managed object rename custom_widget custom_gadget [--yes] [--json]
aic managed object delete custom_gadget [--yes] [--json]
aic managed field add custom_widget.code --type string [--title T] [--required false] [--enum value[:Title] ...] [--default VALUE] [--yes] [--json]
aic managed field edit custom_widget.code --searchable true [--enum value[:Title] ... | --clear-enum] [--default VALUE | --clear-default] [--allow-narrowing] [--yes] [--json]
aic managed field rename custom_widget.code external_code [--yes] [--json]
aic managed field delete custom_widget.external_code [--yes] [--json]
aic managed hook add custom_widget onCreate [--yes] [--json]
aic managed relationship set custom_widget.owner --target alpha_user --forward one [--reverse many] [--reverse-key widgets] [--yes] [--json]
aic managed relationship delete custom_widget.owner [--yes] [--json]
```

Every write accepts `--tenant <name>` and requires `--yes` for a
production-themed tenant. Field and relationship booleans take explicit values
(for example `--viewable false`). Field creation defaults to non-searchable,
viewable, user-editable, and optional; omitted field-edit flags leave that
attribute unchanged. `<object>.<key>` must contain exactly one dot. Every schema
write is recorded for reversal from the TUI history overlay; there is no CLI
undo command.

`--enum` is repeatable and replaces the field's allowed-value set; use
`value:Title` for a display label. On `field edit`, `--clear-enum` removes the
constraint entirely (`field add` rejects it — a new field has none to clear).

`--default` supplies the server-applied value when a record omits the field on
create; it is not UI prefill and also satisfies `required`. The value must match
the field type. The CLI validates this locally because IDM accepts a mismatched
schema default with 200, then the managed object returns 404 forever. Use
`--clear-default` on `field edit` to remove it (`field add` rejects it). For a
`string[]` field, pass JSON such as `'["a","b"]'`.

Removing a value from an existing set requires `--allow-narrowing`, and warns on
stderr even then. Nothing fails at the moment you narrow: records holding a
removed value still read back, and patches to their other properties still
succeed. What breaks is a whole-record `PUT` of such a record — in some other
integration, on a property that code never touched. Adding a value, and
`--clear-enum`, are both widening and need no flag.

---

## `aic sync` — queued sync diagnostics & reconciliation

`sync` diagnoses IDM's persistent asynchronous implicit-sync queue and runs
reconciliations. Queue commands are read-only; this CLI has no queue-clear
operation.

```bash
aic sync mappings [--tenant <name>] [--json]
aic sync queue [--mapping <name>] [--tenant <name>] [--json]
aic sync queue --watch 5 [--mapping <name>] [--json]  # JSON is JSONL when watching
aic sync recon <mapping> [--id <source-id>] [--wait] [--timeout 10m] [--yes] [--json]
aic sync recon-status [<recon-id>] [--tenant <name>] [--json]
```

`mappings` shows each mapping's queued-sync posture and configured poll ceiling.
`queue` labels all totals as estimates: IDM silently downgrades its exact-count
request. Its claim-state numbers are a bounded sample, not an extrapolated
total. `--watch` requires at least two seconds and prints queue depth, signed
drain rate, and ETA as the rate permits.

`recon` writes target data and therefore requires `--yes` for a production
tenant. `--id ... --wait` uses IDM's synchronous one-record form, which is the
only response that includes its per-record failure reason. Completion output
includes records/sec so it can be compared with queued-sync drain rate.

## `aic idm` — local record store & query

Syncs IDM managed-object **records** into a local SQLite store
(`.aic/idmstore/<tenant>.sqlite`, gitignored) so you can query them with SQL —
including joins into nested arrays. Each object becomes a base table
`obj_<type>` (full record JSON in `data`, plus generated columns for top-level
scalar fields), with child tables `obj_<type>__<field>` for arrays and
relationships.

```bash
aic idm objects [--json]                         # list syncable object names (live, from the tenant)
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
aic journey list [--realm alpha] [--json]              # journey names
aic journey pull <name> [--realm alpha]                # tree + nodes → workspace JSON
aic journey push <name> [--realm alpha] [--force]      # push an export back
aic journey delete <name> --force [--realm alpha]      # delete (requires --force)
aic journey using-script <script-uuid> [--realm alpha] [--json] # journeys referencing a script
aic journey nodes [--realm alpha] [--json]             # available node types
aic journey node-schema <nodeType> [--realm alpha]     # a node type's schema (JSON)
aic journey node-template <nodeType> [--realm alpha]   # a starter node config (JSON)
```

---

## `aic oauth` — OAuth2 clients

Realm-scoped. Clients pull/push as JSON under the workspace.

```bash
aic oauth list [--realm alpha] [--json]                 # client ids
aic oauth create <id> [common flags] [--from FILE]      # create from live tenant defaults
aic oauth grant list <id> [--realm alpha]              # grant types on one client
aic oauth grant add <id> <grant>... [--realm alpha] [--yes]
aic oauth grant remove <id> <grant>... [--realm alpha] [--yes]
aic oauth pull <id> [--realm alpha]                     # one client → workspace JSON
aic oauth push <id> [--realm alpha] [--force]           # push a workspace client JSON back
aic oauth delete <id> --force [--realm alpha]           # delete (requires --force)
```

`create` exposes the common client settings (`--name`, repeatable scopes,
redirect URIs, grants/response types, token auth, consent, and lifetimes); run
`aic oauth create --help` for the compact list. Use `--secret-stdin` to supply a
write-only secret, or `--generate-secret` to print a generated secret exactly
once after a successful create. A secret cannot be recovered from AIC later. The
command refuses an existing id unless `--force` is given and requires `--yes`
for a production-themed tenant. **`--force` replaces the client wholesale from
the tenant template — it is not a merge**, so every field you don't pass returns
to its tenant default. To change one setting on a client that already exists,
use pull → edit → push instead.

For less-common settings, pass an OAuth client JSON object with `--from`; flags
override its values while missing fields retain the live tenant template.
`aic oauth pull` output composes directly with this path. For ongoing edits,
continue to use pull → edit → push.

`grant add` and `grant remove` update only
`advancedOAuth2ClientConfig.grantTypes` on an existing client. They are
idempotent: an already-present grant or an absent grant reports no change.
Grant values are checked against the tenant's live OAuth2 client schema when it
is available; if the schema cannot be read, AM performs the validation. The
commands require `--yes` on production-themed tenants. Adding the JWT-bearer
grant emits a security note because a Trusted JWT Issuer with empty
`allowedSubjects` can then mint a token as any user in the realm.

> `*-encrypted` fields are cluster-local and stripped from every client PUT;
> server-managed metadata is also removed and `_rev` is ignored (plain PUT). See
> `docs/api/05-oauth2-oidc.md`.

## `aic jwt-bearer` — Trusted JWT Issuer setup

```bash
aic jwt-bearer setup [--realm alpha] [--tenant NAME]
aic jwt-bearer issuer create <id> --issuer ISS --jwks-from FILE [--realm alpha] [--tenant NAME]
aic jwt-bearer issuer show [<id>] [--realm alpha] [--tenant NAME]
```

`setup` creates or updates the default lower-environment issuer, merges this
install's public key into its shared key set, and stores the private key in the
per-tenant encrypted vault. It is idempotent. All JWT-bearer writes are refused
on production-themed tenants; no confirmation flag overrides that refusal.
`issuer create` imports an existing public JWKS under a named issuer, and
`issuer show` prints one issuer or the realm's issuer list as JSON.

---

## `aic secretmap` — AM secret-label → ESV-secret mappings

Realm-scoped. Re-point AM secret _labels_ (purposes) at existing ESV secrets.

```bash
aic secretmap list [--realm alpha] [--json]            # configured mappings
aic secretmap list-labels [--realm alpha] [--json]     # valid AM secret labels (alias: labels)
aic secretmap get <secret-label> [--realm alpha]       # one raw mapping
aic secretmap set <secret-label> <esv-secret-id> [--realm alpha] [--force]
aic secretmap remove <secret-label> [--realm alpha]    # alias: delete
```

---

## `aic workspace` — typed script workspace scaffold

Scaffold and refresh the local **typed workspace** at `./workspace/<tenant>/`
(one tree per tenant) with `.d.ts` definitions + ESLint/TypeScript config, so
your editor gets full IntelliSense on script bodies.

```bash
aic workspace init                            # scaffold the tenant tree (both realms + idm)
aic workspace update                          # refresh bundled types/config to the latest
```

## `aic script` — typed script workspace sync

Two-way sync of AIC scripts to the workspace. Four script "kinds" sit behind one
engine:

- **AM scripts** — realm-scoped, under `am/<realm>/<type>/` (e.g.
  `decision-node`, `lib`, `oidc-claims`; Groovy scripts aren't synced).
- **IDM custom endpoints** — tenant-global under `idm/endpoint/`.
- **IDM scheduled jobs** — tenant-global under `idm/schedule/` (script-invoking
  schedules only).
- **IDM managed-object hooks** — under `idm/managed/<object>/<hook>.cjs`
  (file-backed hooks are read-only).

### The `<ref>` model

Scripts are addressed by a **full-name** `<namespace>/<name>`, where the
namespace is `alpha`/`bravo` (AM realm), `endpoint`, `schedule`, `sync`, or
`managed` (hook name is `<object>.<hook>`, e.g. `managed/alpha_user.onCreate`).
So you never pass `--kind`/`--realm` to script commands. A bare `<name>`
resolves its namespace from your current directory. A bare namespace (`bravo`,
`endpoint`) means "all of it"; `all` means everything.

### Commands

```bash
aic script list [<ref>] [--json]                # list scripts (each row tagged with its `ref`)
aic script create <ref> --context <ctx> [--from FILE] [--language LANG] [--evaluator-version V] [--description TEXT] [--tenant TENANT] [--yes]
aic script copy <src-ref> <dst-ref> [--tenant TENANT] [--yes]
aic script delete <ref> --force [--tenant TENANT] [--yes]
aic script pull [<ref>] [--force]               # pull; no ref → fuzzy picker
aic script push [<ref>] [--force] [--yes]       # push local edits; no ref → fuzzy picker
aic script sync [<ref>] [--resolve local|remote] # reconcile: push local-only, pull remote-only
aic script watch                                # auto-push each .cjs you save (Ctrl-C to stop)
aic script status [<ref>]                       # in sync / modified / remote / conflict
aic script diff [<ref>] [--local-vs-snapshot | --snapshot-vs-remote]
```

- `create`, `copy`, and `delete` apply only to standalone AM scripts, IDM
  endpoints, and IDM schedules. Managed hooks and sync-mapping scripts are slots
  in their owning configuration documents.
- AM `create` requires `--context`; it accepts either an AM context constant or
  the workspace folder slug (such as `decision-node` or `lib`). `copy` is
  same-tenant only (including alpha-to-bravo cross-realm copies), retains the
  complete source config, and both create/copy pull the server's canonical form
  into the workspace. `create` refuses legacy (`evaluatorVersion: "1.0"`)
  scripts.
- `delete` requires `--force` and retains the local `.cjs` file while removing
  its snapshot/manifest entry. All three lifecycle writes require an initialized
  workspace.
- Scripts are promoted static content: `push`, `sync`, `watch`, `create`,
  `copy`, and `delete` refuse staging and production tenants before making an
  API call. `pull`, `list`, `status`, and `diff` remain available on every
  tenant for promotion verification.

- **Fuzzy picker.** `pull`/`push` with no `<ref>` open an interactive picker
  (type to filter). Lines are marked `!` (local changes) or `-` (not pulled); on
  `push`, locally-changed scripts sort first.
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

> After upgrading the binary, restart the agent (`aic session stop` then
> `aic session login`) so it loads new `Accept-API-Version` headers used by
> AM-script support.

---

## See also

- [README](../README.md) — what the tool is, setup, and the agent model.
- [`docs/api/`](api/) — verified AIC endpoint reference (read before changing
  code that hits a tenant).
