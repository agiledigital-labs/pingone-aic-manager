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
  guard. `--yes` authorizes only the environment; it never grants a separate
  operation-specific safety override.
- **`--force`** skips a safety check specific to the command (e.g. overwriting a
  drifted remote, deleting a journey/client). It's called out per command below.
  On commands that document named guards, bare `--force` means
  `--force=operation`; named forms require `=` and are repeatable. Each form
  grants only its named permission—there is no `all` guard.
- **Output format.** List commands default to kubectl-style tables. Pass
  `--json` on list commands for machine-readable output. Single-resource reads
  and export-style commands still print JSON by default.
- **Project directory.** `aic` roots itself at the nearest ancestor containing
  `.aic/`, so any subdirectory of a project works. Pass the global
  **`--project <dir>`** — or set **`AIC_PROJECT=<dir>`**, which it overrides —
  to run from somewhere else entirely: a script in a sibling repo, an editor
  task, a CI step. Either stands in for the cwd: the walk up to the project root
  and the `workspace/<tenant>/` tenant inference both start there. Pointing one
  at something that is not inside a project is an error rather than a fallback
  to the cwd, so a typo cannot silently act on a different tenant.

  `--project` is `global = true`, so it is accepted before or after the
  subcommand (`aic --project ~/w/x esv list` and `aic esv list --project ~/w/x`
  are the same). It is resolved from raw argv before parsing, because the
  process has to be rooted before `--tenant`'s default can be read out of the
  project's own config.
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
| `aic ctx rm <tenant>`          | Remove a tenant entry and the local artifacts belonging to it. See below.                                      |
| `aic whoami [--tenant <name>]` | Mint and print token info plus the local operator name and host for a context.                                 |
| `aic whoami --token`           | Print **only** the bearer token (for scripting, e.g. `curl -H "Authorization: Bearer $(aic whoami --token)"`). |

The normal `whoami` output includes `operator: <name> on <host>`. When the name
has not been saved yet, the line says it is unset and points to
`aic settings set operator.name <name>`. `--token` remains exactly one bare
token on stdout.

Unlike the other `ctx` verbs, `rm` needs an unlocked agent: it reads the vault
to find out what the tenant owns, and withdraws a signing key from the tenant
itself.

#### `aic ctx rm` — remove a tenant

| Flag            | Effect                                                                                     |
| --------------- | ------------------------------------------------------------------------------------------ |
| `--dry-run`     | Print the plan and exit, changing nothing.                                                 |
| `--json`        | Print the plan as JSON and exit, changing nothing.                                         |
| `--delete-keys` | Accept every offered artifact and skip all prompts, including the typed-name confirmation. |
| `--yes`         | Confirm a write to a production-themed tenant.                                             |

The command prints a plan first, then asks `[Y/n]` per artifact, then requires
the tenant name typed back. Each row is one of four states:

- **offered**, defaulting to on — or to **off** when the credential's recorded
  provenance says you supplied it rather than `aic` minting it;
- **absent** — nothing stored, so no choice is offered;
- **refused** — a _surviving_ tenant entry still needs it. `--delete-keys`
  forces past a prompt, **never** past a refusal;
- **implied** — the workspace directory contains the sync state, so accepting
  the workspace takes it regardless.

Refusal is matched on resource identity, not on the tenant name: the same
`sa_id`, the same log `api_key_id`, the same signing kid on the same `base_url`,
or a colliding sanitised store filename. Two entries can point at one AIC tenant
and share some credentials while differing in others, which is why the plan
prints those identifiers — with two similar entries they are the only way to
tell which one you are about to remove.

**Two things `aic` cannot delete, and will report instead.** The service account
and the log API key both need an admin-user bearer; a service-account bearer
gets 403 on `DELETE /openidm/managed/svcacct/{id}` and on `DELETE /keys/{id}`
(`docs/api/00-auth.md`, `docs/api/08-logs.md`). So purging those removes the
local credential only, and the run ends by naming the `sa_id` and `api_key_id`
to delete in the AIC console. That is the expected end of a successful run, not
an error.

The Trusted JWT Issuer is different: the issuer is **shared**, holding one
signing key per install, so `rm` withdraws only this install's kid and never
deletes the issuer. If that remote step fails the local purge still completes
and the kid is reported — it stays trusted by the tenant until you remove it in
the console.

A pre-delete backup is written to
`.aic/backups/tenant-<name>-<YYYYMMDD>T<HHMMSS>Z.json` at mode 0600. It holds
the config entry and the identifiers, and **no secret material** — the vault may
be encrypted, and a plaintext private key beside it would defeat that. The
backup makes an accidental deletion reconstructible (you can re-onboard with the
same values), not reversible. If you want to keep the credentials themselves,
export them before deleting: once the entry is gone, no command can name the
tenant to reach its vault entries.

Execution removes the `[[tenant]]` entry **last**. If anything before it fails,
the entry stays and the whole removal can be retried; the command exits non-zero
and says so.

### aic auth — mint a token as an end user

    aic auth --as-id <uuid> --client-id <id> [--client-secret-stdin] [--client-auth <method>] [--scope S]...
    aic auth --as-username <name> --client-id <id> [--client-secret-stdin] [--client-auth <method>] [--scope S]...
    aic auth ... --token

Exactly one of --as-id and --as-username is required. Usernames are resolved to
their IDM managed-object UUID before signing. The client secret is read from
stdin only when --client-secret-stdin is supplied; omitting it sends a public-
client request with no client credential. Secrets are never accepted as argv or
environment values. `--client-auth` accepts `client-secret-post` (the default)
and `client-secret-basic`. Set it to match the OAuth client's
`tokenEndpointAuthMethod` if you want the request to be strictly conformant; AM
was observed accepting either method regardless (see
`docs/api/17-jwt-bearer-user-tokens.md`), so a mismatch is not known to fail.
`private-key-jwt` is reserved for a future extension and is not accepted yet.
The command requires a key from `aic jwt-bearer setup`. On a production-themed
tenant it reads the Trusted JWT issuer first and refuses to mint unless that
issuer names the users it may act for; elsewhere it does not read the issuer at
all. It takes no `--yes`, because minting a token is not a tenant write.

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
aic esv secret destroy esv-my-secret 2 [--force] [--yes] # irreversible — destroy one version
aic esv secret delete  esv-my-secret [--force] [--yes]   # irreversible — delete the secret
```

**Value sources** (for `create` / `add-version`), in priority order:

1. `--value-file <path>` — read from a file (one trailing newline stripped).
2. `--value-stdin` — read from stdin (e.g.
   `printf 'secret' | aic esv secret add-version … --value-stdin`).
3. interactive no-echo prompt (default if none given).

`--value <v>` exists for scripting but is **discouraged** — it leaks into shell
history and `ps`. `create` is create-only (PUT); change a value with
`add-version`, which becomes the active version.

Destroying a version and deleting a secret require a typed `yes` confirmation
on an interactive terminal. Bare `--force` replaces that operation
confirmation for unattended use. Production independently requires `--yes`:
neither flag implies the other, so unattended production deletion needs both.

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
| `aic logs key create [--tenant <name>] [--cookie-name <name>] [--yes]` | Mint a new key pair from an admin session, then store it. Prompts for the AM session cookie value if needed; production requires `--yes` before any session input is read. |

### Remote fetch

| Command                                                                                                                                            | What it does                                                                                     |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `aic logs sources [--tenant <name>] [--json] [--output <path>]`                                                                                    | List available log source ids. `--json` prints the list as JSON; `--output` writes it to a file. |
| `aic logs tx <transaction_id> [--tenant <name>] [--source <csv>] [--output <path>] [--wait] [--timeout <secs>]`                                    | Fetch all events for one transaction id. `--source` narrows to a comma-separated source list.    |
| `aic logs range <begin> <end> [--tenant <name>] [--source <csv>] [--query <crest>] [--output <path>]`                                              | Fetch events in an ISO-8601 time range. `--query` adds an optional CREST filter.                 |
| `aic logs query <filter> [--begin <iso>] [--end <iso>] [--tenant <name>] [--source <csv>] [--output <path>]`                                       | Run a CREST filter over the logs API. Defaults to the most recent 24 hours.                      |
| `aic logs grep <pattern> [--since <duration> \| --begin <iso> --end <iso>] [--source <csv>] [--tenant <name>] [--output <path>]`                    | Fetch live logs and match a case-sensitive substring within each normalized payload.            |
| `aic logs tail [--source <csv>] [--pattern <text>] [--tenant <name>]`                                                                              | Follow live logs until Ctrl-C, optionally filtering on the same payload substring match.         |

`logs grep` defaults to `--since 15m`; durations accept positive whole seconds,
minutes, or hours (`30s`, `5m`, `1h`). `--begin` and `--end` are an alternative
fixed ISO-8601 window and must be supplied together. Matches form one JSON array
on stdout, or in `--output`; pages are filtered and written as they arrive rather
than accumulating the whole window in memory.

`logs tail` begins with the most recent 15 seconds and then requests contiguous
windows until Ctrl-C. Each matched event is one compact JSON value on stdout
(JSON Lines). Startup, empty-poll/filter status, and the Ctrl-C acknowledgement
go to stderr, so redirecting stdout produces a clean event stream. There is no
separate polling-interval flag: all log requests already pass through the
client's 1.05-second API throttle.

Both commands inspect `payload`, which the live API returns as either a raw
string (especially `idm-core`) or a JSON object. Objects are serialized to JSON
before the case-sensitive substring match; the original event shape is emitted.
Events are ordered by the documented top-level `timestamp` within each fetched
page before matching and emission.

Log ingestion can lag tens of seconds behind the request, so a short
`aic logs tx` result is not proof the script never ran. After every `tx` fetch
the CLI looks for the trailing `AM-ACCESS-OUTCOME` event. If it is missing, a
note goes to **stderr** (stdout stays the JSON event list, including when
redirected or written with `--output`):

```
note: no AM-ACCESS-OUTCOME event yet for transaction <id> — logs can lag
tens of seconds behind the request; retry, or pass --wait
```

`--wait` polls the same transaction query until that event arrives or
`--timeout` seconds elapse (default 60). Progress lines go to stderr. Timeout
does not fail the command: the events from the last poll are still written.
`--timeout` is accepted without `--wait` and ignored. Range and query have no
equivalent completion signal, so they have no `--wait`.

### Local store

> **Build-gated, and not in the binary you downloaded.** The three commands
> below need the `logs-store` cargo feature. The release workflow runs
> `cargo build --release --locked --bin aic` with no features, so a released
> `aic` does not have them: `aic logs search` fails with
> `error: unrecognized subcommand 'search'`, which says nothing about a feature
> and reads as a missing command. Build your own with:
>
> ```sh
> cargo build --release --features logs-store
> ```
>
> Everything earlier in this section is in every build. DuckDB is why these
> three are opt-in.

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
aic managed get alpha_user [--fields GLOB] [--hook-sources]
aic managed object create custom_widget [--title T] [--description D] [--yes] [--json]
aic managed object rename custom_widget custom_gadget [--yes] [--json]
aic managed object delete custom_gadget [--yes] [--json]
aic managed field add custom_widget.code --type string [--title T] [--required false] [--enum value[:Title] ...] [--default VALUE] [--yes] [--json]
aic managed field edit custom_widget.code --searchable true [--enum value[:Title] ... | --clear-enum] [--default VALUE | --clear-default] [--force] [--yes] [--json]
aic managed field rename custom_widget.code external_code [--yes] [--json]
aic managed field delete custom_widget.external_code [--yes] [--json]
aic managed hook add custom_widget onCreate [--yes] [--json]
aic managed relationship set custom_widget.owner --target alpha_user --forward one [--reverse none|one|many] [--reverse-key widgets] [--yes] [--json]
aic managed relationship delete custom_widget.owner [--yes] [--json]
```

`managed get` prints one object's definition as JSON. Inline hook `source`
bodies are omitted by default and replaced with the argument `aic script pull`
takes (`<script: managed/alpha_user.onUpdate>`), because those sources are
already first-class workspace scripts. Pass `--hook-sources` to include the
real source. `--fields` is a repeatable, both-ends-anchored glob over
`schema.properties` keys (`*` any run, `?` one character; case-sensitive);
`schema.order` and `schema.required` are narrowed to the surviving keys. If no
pattern matches, the command fails rather than printing an empty properties
map. A filtered document also notes on stderr how many properties are shown,
and names any pattern that matched nothing.

Every write accepts `--tenant <name>` and requires `--yes` for a
production-themed tenant. Field and relationship booleans take explicit values
(for example `--viewable false`). Field creation defaults to non-searchable,
viewable, user-editable, and optional; omitted field-edit flags leave that
attribute unchanged. On `relationship set`, an omitted `--reverse` likewise
**keeps** whatever the relationship already declares — including a reverse that
names a property the target object does not have — and `--reverse-key` defaults
to the declared name. It is `none` only when the relationship is being created.
Removing a reverse therefore takes an explicit `--reverse none`, which also
deletes the property from the target object. `<object>.<key>` must contain exactly one dot. Every schema
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

Removing a value from an existing set requires bare `--force`, and warns on
stderr even then. This replaces the removed `--allow-narrowing` spelling.
`field add` and `relationship set` accept neither spelling because narrowing
does not apply to those commands. Nothing fails at the moment you narrow:
records holding a removed value still read back, and patches to their other
properties still succeed. What breaks is a whole-record `PUT` of such a record
— in some other integration, on a property that code never touched. Adding a
value, and `--clear-enum`, are both widening and need no flag.

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
aic journey push <name> [--realm alpha] [--force] [--yes] # push; production requires --yes
aic journey delete <name> --force [--realm alpha] [--yes] # delete; production requires --yes
aic journey using-script <script-uuid> [--realm alpha] [--json] # journeys referencing a script
aic journey nodes [--realm alpha] [--json]             # available node types
aic journey node-schema <nodeType> [--realm alpha]     # a node type's schema (JSON)
aic journey node-template <nodeType> [--realm alpha]   # a starter node config (JSON)
```

---

## `aic policy` — AM policies, policy sets and the PDP

Realm-scoped, CLI-only (no TUI tab). Three collections — resource types, policy
sets (`applications` on the wire) and policies — plus `eval`, which asks the
policy decision point and then explains the answer.

```bash
aic policy list [--set <set>] [--realm alpha] [--json]
aic policy show <name> [--realm alpha]
aic policy pull <name> | --all [--set <set>] [--realm alpha]
aic policy push <name> [--force] [--realm alpha] [--yes]
aic policy rm   <name> --force [--realm alpha] [--yes]

aic policy set list|show|pull|push|rm …          # policy sets, same shape
aic policy rt  list|show|pull|push|rm …          # resource types, same shape
aic policy types [--realm alpha] [--json]        # subject + condition catalogs

aic policy eval --set <set> --resource <url> [--resource <url> …] \
  [--action <name> …] \
  [--subject-jwt <token> | --subject-jwt-file <path> | --subject-sso <token>] \
  [--env scope=orders.read …] [--json] [--no-explain] [--realm alpha]
```

`pull` writes `workspace/<tenant>/policy/<realm>/{policies,sets,resourcetypes}/`
with a `.snapshots/` sibling, and strips the fields AM writes itself (`_rev`,
`createdBy`, `creationDate`, `lastModifiedBy`, `lastModifiedDate`, `editable`)
so a diff shows only what an operator authored. `push` compares content, not a
revision: an unchanged remote is a no-op, a remote that has drifted **back** to
the snapshot is pushable, and any other drift is refused until you re-pull or
pass `--force`. The verb asymmetry is handled for you — resource types create
with `PUT`, policies and policy sets only with `POST ?_action=create`.

### `aic policy eval`

The reason this vertical exists. AM answers a denied request with
`actions: {}`, which means "no policy applied" and covers a resource that
matched nothing, a subject that failed, and a condition that failed — with no
way to tell them apart. `eval` reads the set, its resource types and its
policies and says which it was:

```
$ aic policy eval --realm bravo --set CapTokenDemo \
    --resource https://shop-api.demo:443/payments/9 --action refund \
    --subject-jwt-file ./cap.jwt

RESOURCE                              DECISION
https://shop-api.demo:443/payments/9  {} no policy applied

https://shop-api.demo:443/payments/9
  - policy CapTokenDemo_PaymentsRefund wants claim demoRoles="payments.admin";
    the token has ["orders.approver","orders.reader"]
  - policy CapTokenDemo_PaymentsRefund wants claim scope="payments.refund";
    the token has ["orders.approve"]
  - the resource matched policy CapTokenDemo_PaymentsRefund — so the subject or
    a condition is what failed (subjects: AND, JwtClaim; conditions: none)
```

It also names the traps that produce a silent `{}`: a query string in the
resource (AM matches it as part of the resource, so `…/orders/1?x=1` misses
`…/orders/*`), a resource that names no port on a scheme AM cannot default, an
action no resource type declares, an `AuthenticatedUsers` subject under a `jwt`
subject (which never matches), and an `OAuth2Scope` condition with no
`environment.scope`. `--action` sharpens all of this; without it, `eval` only
explains a completely empty answer.

`--subject-jwt-file` keeps the token out of shell history. The claims it prints
are **decoded, not verified** — which is also all AM does: the PDP checks
neither the signature nor the expiry of `subject.jwt`, so a resource server must
verify locally before it evaluates. See `docs/api/21-am-policies.md`.

---

## `aic role` — IDM internal roles

Internal roles are tenant-global. Their `_id`, rather than their display `name`,
is what IDM authorization configuration references. Creating through this
command makes the caller-chosen id the default name as well.

```bash
aic role list [--json]
aic role show <id> [--json]
aic role create <id> [--name <name>] [--description <text>] [--yes]
aic role delete <id> [--force] [--yes]
aic role privilege list <role-id> [--json]
aic role privilege add <role-id> --path managed/alpha_user --permissions VIEW,UPDATE --attr mail:rw --attr userName:ro [--privilege-name <name>] [--actions action1,action2] [--yes]
aic role privilege rm <role-id> --path managed/alpha_user [--yes]
```

`create` refuses an existing id because IDM's `PUT` is a destructive full
replace; use `role privilege add` to amend privileges. `privilege add` validates
the path and attribute names against the tenant's managed-object schema, then
replaces an existing privilege with the same path or appends a new one. Each
`--attr` uses `name:ro` or `name:rw`. Known permissions are `VIEW`, `CREATE`,
`UPDATE`, `DELETE`, and `ACTION`; other values warn and proceed because AIC does
not publish an authoritative enum. Privilege edits use the revision read with
the role; a concurrent modification is reported and left untouched instead of
being overwritten. Deletion prompts by default, while `--force` skips
confirmation.

All role mutations require `--yes` when the selected tenant is configured as
production. `--force` on delete only skips its separate deletion prompt; it does
not confirm the production environment.

---

## `aic access` — IDM authorization rules

`config/access` is a tenant-global, ordered array of grant rules. Rules are
OR-ed: adding a rule can only grant access, while narrowing or removing an
existing grant can revoke access and lock operators out.

```bash
aic access list [--json] [--role R] [--pattern P] [--method M] [--duplicates] [--warnings]
aic access show <index-or-digest> [--json]
aic access get [--out FILE]
aic access add --pattern P --roles R --methods M [--actions A] [--custom-authz S] [--exclude-patterns S] [write flags]
aic access edit <index> [--pattern P] [--roles R] [--methods M] [--actions A] [--custom-authz S] [--exclude-patterns S] [--clear-actions] [--clear-custom-authz] [--clear-exclude-patterns] [write flags]
aic access rm <index>... [write flags]
aic access apply <file> [write flags]
```

All commands accept `--tenant <name>`. The write flags are `--if-digest <hex>`,
`--yes`, `--dry-run`, and repeatable `--force[=<guard>]`. Bare `--force`
confirms the displayed change summary; `--force=backup` skips the pre-write
backup. They are independent, so unattended use that needs both spells both
flags. `--yes` only confirms a production environment. `list` prints the
whole-document digest used by `--if-digest`; a write with a stale digest is
refused regardless of force. It also prints each rule's 0-based index and
8-character rule digest. Writes use the index because duplicate rules are
legal—several byte-identical entries make “replace this entry” ambiguous by
content. `show` may use either address; a digest that identifies duplicates
shows every matching entry, and `list --duplicates` filters to all members of
duplicate groups.

`list` prints one indented block per rule, headed by its index and rule digest,
rather than one row per rule. A key the rule omits gets **no line**, so an
absent `actions` is visually distinct from `actions: ""` — six of the sandbox's
65 rules legitimately omit the key, and a single-row table rendered both as
blank. `customAuthz` is clipped to one line; use `aic access show <address>` or
`--json` for the body. Role paths print in full, so they paste straight back
into `--roles`.

`list` validates the whole document but **counts** its warnings rather than
printing them — the sandbox's own 65 rules produce 28, so spelling them out
buries the rule blocks and trains you to ignore the line that matters.
`--warnings` spells them out. Write verbs are the opposite: their warnings are
already scoped to the rules the command touched, so those always print.

Before a write, the fetched document is saved with mode 0600 at
`.aic/backups/access-<tenant>-<UTC>.json` unless `--force=backup` is supplied.
If an attempted backup fails, the write is refused even with bare `--force`.
`--dry-run` prints the rule-level change summary without writing, confirming,
or creating a backup. Writes prompt after showing the summary unless bare
`--force` is supplied; global `--no-prompt` therefore requires `--force` for a
real write (and production additionally requires `--yes`).

Migration: replace the removed `--no-backup` spelling with
`--force=backup`. If an old unattended command used `--yes` to accept the
summary, add bare `--force`; retain `--yes` only when production consent is
needed.

The backup is taken **first**, before validation and before the prompt — so a
refused validation or a declined confirmation still leaves a backup file behind.
That is deliberate: the backup exists to survive a write that goes wrong, not to
record that one happened. Backups are never pruned; delete them yourself.

**`aic access` writes are not in the undo log.** Unlike `aic managed`, which
tells you to undo from the TUI history overlay, the undo log is TUI-only — the
backup file is the entire safety net here, which is why it is taken before
anything else and why its path is printed.

Access-tab writes get **both**: a mode-0600 backup before the `PUT`, same as
here, and an undo entry that appears in the history overlay. The tab has no
backup-bypass equivalent, so a failed backup blocks the write there too.

`aic access get --out access.json`, edit the file, then
`aic access apply access.json` is the guarded hand-edit workflow. Restore a
backup through the same path:
`aic access apply .aic/backups/access-<tenant>-<UTC>.json` backs up the current
document, validates, summarizes, and then restores the saved one.

`--role`, `--pattern` and `--method` are **exact** matches, not globs or
substrings: `--pattern managed/alpha_user` finds nothing on a tenant whose rules
say `managed/alpha_user/*`. `--role` and `--method` match one entry of the
comma-separated list, so `--method read` finds a rule whose `methods` is
`read,query`.

---

## `aic oauth` — OAuth2 clients and provider configuration

Realm-scoped. Clients pull/push as JSON under the workspace.

```bash
aic oauth list [--filter TEXT] [--no-dynamic] [--realm alpha] [--json]
aic oauth get <id> [--realm alpha] [--tenant T] [--json]  # one client, read-only
aic oauth provider get [--realm alpha] [--tenant T] [--json] # realm-wide OAuth2/OIDC provider
aic oauth exchange list [--all] [--realm alpha] [--json]  # who takes part in RFC 8693 token exchange
aic oauth create <id> [common flags] [--from FILE]      # create from live tenant defaults
aic oauth grant list <id> [--realm alpha]              # grant types on one client
aic oauth grant add <id> <grant>... [--realm alpha] [--yes]
aic oauth grant remove <id> <grant>... [--realm alpha] [--yes]
aic oauth pull <id> [--realm alpha]                     # one client → workspace JSON
aic oauth push <id> [--realm alpha] [--force] [--yes]   # push; production requires --yes
aic oauth diff <id> [--local-vs-snapshot | --snapshot-vs-remote]  # compare two versions
aic oauth delete <id> --force [--realm alpha] [--yes]   # delete; production requires --yes
```

`provider get` prints a compact realm-wide configuration summary, resolving
script ids to names the same way `get` does. It always
shows a row for both the provider `grantTypes` and `tokenExchangeClasses` —
`<absent>` when the tenant does not set one — because configured exchangers do
not themselves enable the token-exchange grant. It also derives a direct
`token-exchange granted` yes/no row, from `grantTypes` alone. Arrays inside the
known configuration groups expand to one value per row and the `[Empty]`
sentinel prints as `<not set>`; a group this command does not know about, and an
object-valued plugin setting, still print as a single JSON cell. Token-exchange
class mappings drop their repeated token-type URN and Java-package prefixes.
That rewrite is purely structural, so a mapping that does not parse is printed
in full — but one naming an unfamiliar token type or exchanger class is
shortened like any other. `--json` prints the same document the API returned,
re-serialised rather than passed through byte-for-byte.

`list` prints one row per client: id, client name, type, status and grant
types. A field the tenant does not set shows as `-` — some clients really do
have `status: null`. `--filter TEXT` keeps the clients whose id **or** client
name contains TEXT, case-insensitively; both, because the two disagree in
practice and either can be the one you remember.

`--no-dynamic` hides clients whose id is a bare UUID, which is what AM mints
for a client registered through dynamic registration — on a tenant with a
thousand DCR clients that is the flag that makes the list readable. It tests
the **id**, not the provenance: AM records nothing saying a client came from
DCR, so a hand-made client given a UUID id is hidden too, and a dynamic
registration that supplied its own `client_id` is not. That is why a filtered
listing always reports how many rows were hidden and by which flag. Verified
2026-09-10 against a real dynamic registration: the client AM created carried a
UUID `client_id`, and `--no-dynamic` hid exactly that row.

`--json` still prints ids alone, not the new columns, so anything piping it
into another command keeps working.

`exchange list` answers the question RFC 8693 configuration is spread too thin
to answer by reading: **who may act for whom.** The realm decides whether the
grant exists at all, which token types have an exchanger, and which script
stamps `may_act` by default; each client decides whether it holds the grant
(making it an **actor**) and whether it stamps `may_act` on the tokens it
issues (making it a usable **subject**). One client can be both. By default a
client is listed when it says something the realm header does not: it holds
the grant (or that could not be determined), it configures a may-act script,
it has a **live** override block while the realm sets a may-act script — an
exception to the realm-wide one — it sets its own audience values or a
non-zero auth level, or something in its configuration could not be read. The
tally says how many were hidden; `--all` lists every client.

The subject half is the **effective** answer, not the configured one, and the
cases differ:

- a live client override with a script — shown as `subject`;
- **no live override** while the realm sets `accessTokenMayActScript` — shown
  as `subject (realm)`. A realm script makes *every* such client a subject, so
  the header says it once rather than the table saying it a thousand times;
- a `(dormant)` override — configured under a `providerOverridesEnabled` that
  is not `true`, so it stamps nothing and the realm's script applies instead.
  Without a realm script, that client's tokens cannot be exchanged at all;
- a **live** override that sets no script — `providerOverridesEnabled: true`
  with the field on `[Empty]`. This is not silence: enabling the block stops
  inheritance for every field in it at once, so the realm's script does *not*
  apply and nothing stamps the claim.

Because the default view lists clients with exchange configuration **of their
own**, a client that is a subject only by inheritance does not appear without
`--all`. The header line is what tells you it exists.

`MAY-ACT SCRIPT` names the script per field — `access` and `oidc` stamp the
claim on different token types, and they collapse to `access+oidc` when they
name the same script. It prints a resolved script's **name** alone to keep the
row inside a terminal; an *unresolved* id keeps its full UUID, because that
case is a finding, and `--json` carries every id regardless. `AUTH_LEVEL` is
`tokenExchangeAuthLevel` verbatim. `0` is AM's default; what a non-zero value
does is **not verified here** — the name and AM's use of "auth level"
elsewhere suggest a floor on the subject token, and
`docs/api/22-token-exchange.md` records it as untested. The column reports the
number, not an interpretation of it. `AUDIENCE` is
`allowedResourceServerAudienceValues` — empty means the client cannot be asked
for an `audience` at all. `ACCEPT_AUD` is the client's override of
`acceptAudienceParametersInTokenExchangeRequests`, which lives in the
**override** block and so is governed by the master switch (the other two are
not). It has four values, because three of them are not the same answer:
`yes`/`no` when the live block decides it; `realm` when the block is dormant,
in which case the header's `realm acceptAudienceParameters` line is what
applies; `block default` when the block is live but the field absent, where the
override block's own default governs and this listing never sees it; and `?`
when the master switch or the field itself could not be read. Worth reading with `AUDIENCE`,
because a client that accepts no audience parameters silently **ignores** an
`audience=` it is sent rather than rejecting it.

The warnings are why the command exists. Each names a prerequisite that is
visibly unmet, and every one of them surfaces at the token endpoint as the same
message — `unsupported_grant_type` for a missing grant, and
`invalid_request: Invalid token exchange.` for the rest:

- clients hold the grant but the realm does not grant the type;
- the realm grants it but no client holds it;
- **nothing stamps `may_act`** — no live client override and no realm
  `accessTokenMayActScript`. Token exchange is deny-by-default: without the
  claim naming the acting client, every exchange is refused;
- a may-act script sitting under a `providerOverridesEnabled` that is not
  `true`;
- the grant is on but no `tokenExchangeClasses` are configured.

That list is **not** every way an exchange fails, and the difference matters
when you are debugging one. A may-act script that names the wrong actor or
throws, a `tokenExchangeAuthLevel` the subject token does not reach, an
`audience` the acting client does not allow, an unconfigured subject/requested
token-type *pair*, and a forged subject token are all invisible here and all
produce the same message. Clearing these warnings narrows the search; it does
not end it. In particular there is no field naming *which* actor a subject
permits — the may-act script decides that at mint time — so the relationship is
only fully readable by reading the script this command names.

A projected field that is **present with an unusable type** is reported, and
the answer it feeds is withheld rather than defaulted — `?` in a cell, `null`
in JSON beside a `…Readable: false`, never the `-` that means "not set". It matters here more
than elsewhere: a `grantTypes` that stopped being an array would quietly turn
an actor into a bystander in the one view whose job is to say who can act, and
a warning printed under "no client holds the grant" does not make that sentence
true. So an unreadable grant list gives `actor?` and suppresses the no-actor
finding; an unreadable `providerOverridesEnabled` gives `subject?` and
suppresses the deny-by-default finding; the realm document gets the same
treatment, where it matters more rather than less. The same applies to a
listing page that comes back with an unusable `_id` or `pagedResultsCookie`:
both are refused rather than skipped, because a listing that quietly loses
rows or stops early is how an incomplete population becomes a confident
statement about every client. An *absent* field stays silent, because absent
really does mean not set.

`--json` carries the realm's half, the projected rows and the findings. The
warnings also go to stderr, so a piped `--json` loses nothing.

Two JSON fields need care. `acceptAudienceParametersOverride` is the
**client's override**, never the runtime value — with the block dormant the
realm's `realmAcceptAudienceParameters` governs, and with the block live but
the field absent the block's own default does, which this projection never
sees. And `tokenExchangeGranted`, `actor` and `overridesLive` are all
nullable: `null` means the document did not say, which is not `false`. The
table spells that `actor?` / `subject?`.

`get` prints one client as a compact table and **writes nothing** — reading a
client used to mean `pull`, which drops a JSON file in the workspace and
overwrites the snapshot as a side effect. It shows what the client is (id,
status, name, type), how it authenticates (`tokenEndpointAuthMethod`,
`subjectType`), and what it may ask for (grant types, response types, scopes,
default scopes, redirect URIs, implied consent, the three lifetimes). Array
fields print one value per row. `--json` prints the document the API returned.

The last section is `overrideOAuth2ClientConfig`, whose 29 keys are only
meaningful alongside one of them: `providerOverridesEnabled` is a master
switch, so the section header says whether the block runs at all rather than
echoing a boolean — `in effect` when the switch is true, `dormant` when it is
false **or absent** (the switch has to be `true`; a missing one leaves the
realm in charge).

The rows are the same either way, deliberately. A dormant
`statelessTokensEnabled: true` is exactly what you need to see before enabling
the block to attach one script, because flipping the switch makes every value
in it live at once — including ones you never set. Entries that say "inherit"
(the `[Empty]` sentinel, `null`, an empty array, a `…PluginType` of
`PROVIDER`, a `…Class` still on AM's `Default…` implementation) are suppressed
and **counted** on a final row, so nothing is dropped without saying so.
A script id prints as `name (uuid)` where the realm has a script with that id,
and as the bare UUID where it does not. Only fields that hold a script id are
rewritten — a scope or a client id that happened to be UUID-shaped keeps its
value. The lookup covers **every** script in the realm, including the Groovy
and product-internal ones `aic script list` hides, because an override can
legitimately point at one and resolving it to nothing would report an existing
script as missing — an id naming a script that is not
there is a finding, and replacing it with a placeholder would remove the value
you need to chase it. Resolution needs a second request; if that fails you get
the UUIDs and a warning saying why.

After the table, `get` warns on stderr about each `…Script` that cannot run:
one set while its `…PluginType` is still `PROVIDER` (or `JAVA`) is ignored by
AM, and a `…PluginType` of `SCRIPTED` with no script runs nothing. Both are
accepted silently by the tenant, which is what makes them expensive. The
may-act fields have no `…PluginType` companion — setting the id is enough —
and are correctly never reported. A warning from a dormant block is prefixed
`(dormant)`: it is not biting yet, and enabling the block is what would make
it bite.

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

Create writes `tokenEndpointAuthMethod: client_secret_post` explicitly so the
result works with `aic auth`'s default. That is deliberately **not** AM's own
template default (`client_secret_basic`), nor the method RFC 6749 §2.3.1 prefers
— it is chosen so the two commands agree without a flag. Override it with
`--token-endpoint-auth-method <value>`; the value is checked against the live
tenant schema when the schema exposes an enum. A value supplied by `--from` wins
over the create default, while an explicit flag overrides the seed.

For less-common settings, pass an OAuth client JSON object with `--from`; flags
override its values while missing fields retain the live tenant template.
`aic oauth pull` output composes directly with this path. For ongoing edits,
continue to use pull → edit → push.

`grant add` and `grant remove` update only
`advancedOAuth2ClientConfig.grantTypes` on an existing client. They are
idempotent: an already-present grant or an absent grant reports no change. Grant
values are checked against the tenant's live OAuth2 client schema when it is
available; if the schema cannot be read, AM performs the validation. The
commands require `--yes` on production-themed tenants. Adding the JWT-bearer
grant emits a security note because a Trusted JWT Issuer with empty
`allowedSubjects` can then mint a token as any user in the realm.

`diff` renders two versions of one client through `git diff`, so your pager and
colour theme apply and `aic oauth diff <id> | <tool>` still pipes plain unified
diff. Three versions exist and it compares two of them: your workspace file
(`local`), the copy `pull` recorded (`snapshot`), and the tenant's current
document (`tenant`). The default is tenant-vs-local — what a `push` would
change. `--local-vs-snapshot` is your own edits, and makes no tenant request.
`--snapshot-vs-remote` is drift on the tenant since you pulled: the comparison
`push` refuses on.

Both sides are normalised exactly the way the drift check normalises — `_rev`
stripped, keys sorted, `-0.0` and `0.0` collapsed — so `diff` and `push` do not
disagree about whether a client changed. Secret values are replaced by their full
SHA-256 — the `*-encrypted` blobs AM returns, and a plaintext `userpassword`,
which only ever appears on the local side because AM reads that field back as
`null`. `pull` already writes both to the workspace, but a rendered diff also
reaches your scrollback, your pager's history and any CI log, and a digest
still changes when the secret is rotated. The OAuth tab masks the same fields
in its detail pane, everywhere in the document.

A side that does not exist is reported on stderr and rendered as empty, and a
comparison where **neither** side exists (a mistyped client id) fails rather
than reporting the two absences as identical. `pull` is suggested as the remedy
only when no **local** side would be lost by running it — it rewrites both the
local file and the snapshot, and never the tenant, so it is the right advice
when the client is simply not pulled yet and the wrong advice when your edits
or your snapshot are the surviving side. In that case the note says what
`pull` would cost instead of naming it.

`push` prints the relevant diff before it refuses. On remote drift that is
snapshot-vs-tenant — what changed under you; with no snapshot at all it is
tenant-vs-local — what `--force` would overwrite. Rendering failures (no `git`
on PATH) degrade to a warning so the refusal itself still reaches you.

> `*-encrypted` fields are cluster-local and stripped from every client PUT;
> server-managed metadata is also removed and `_rev` is ignored (plain PUT). See
> `docs/api/05-oauth2-oidc.md`.

## `aic jwt-bearer` — Trusted JWT Issuer setup

```bash
aic jwt-bearer setup [--id UUID]... [--username NAME]... [--realm alpha] [--tenant NAME] [--yes]
aic jwt-bearer issuer create <id> --issuer ISS --jwks-from FILE [--id UUID]... [--username NAME]... [--realm alpha] [--tenant NAME] [--yes]
aic jwt-bearer issuer show [<id>] [--realm alpha] [--tenant NAME]
aic jwt-bearer subjects list [--issuer ID] [--realm alpha] [--tenant NAME] [--json]
aic jwt-bearer subjects add (--id UUID | --username NAME)... [--issuer ID] [--realm alpha] [--tenant NAME] [--yes]
aic jwt-bearer subjects rm  (--id UUID | --username NAME)... [--issuer ID] [--realm alpha] [--tenant NAME] [--yes]
aic jwt-bearer key list [--realm alpha] [--tenant NAME] [--json]
aic jwt-bearer key remove <KID> --force [--yes] [--realm alpha] [--tenant NAME]
aic jwt-bearer key rotate [--realm alpha] [--tenant NAME] [--yes]
aic jwt-bearer key export [--tenant NAME] [--out FILE]
aic jwt-bearer key import <FILE> [--realm alpha] [--tenant NAME] [--force]
```

`setup` creates or updates the default lower-environment issuer, merges this
install's public key into its shared key set, and stores the private key in the
per-tenant encrypted vault. It is idempotent.

`subjects list`, `add` and `rm` edit the issuer's `allowedSubjects` — the list
of users it may mint for, and the feature's entire security boundary. `--id` and
`--username` are repeatable and may be mixed; a username is resolved to the
user's `_id` before it is written, because AM matches the raw `sub` claim and a
username in the list would never match. An empty or whitespace-only subject is
refused: a list holding only blanks is treated by AM as no restriction at all.
`--issuer` targets a named issuer, defaulting to the shared `aic-agent` one. A
subject edit never disturbs the published key set.

**On production-themed tenants, no write may leave an issuer unrestricted.**
`setup` and `issuer create` take `--id`/`--username` so a first run can supply
the list; `key rotate` is refused against an issuer that is not already
restricted; `subjects rm` will not remove the last real subject. `key remove` is
exempt — withdrawing a signing key reduces capability, and the tenant where a
leaked key matters most is the one where revoking it must stay available.
`aic auth` reads the issuer on production only, and refuses to mint against an
unrestricted one. All of these still take `--yes` like any other production
write. Reads and local key transfer (`key list`, `key export`, `key import`) are
not gated at all. `issuer create` imports an existing public JWKS under a named
issuer, and `issuer show` prints one issuer or the realm's issuer list as JSON.
`key export` writes the tenant's private signing JWK either to stdout or to a
new mode-600 `.jwk` file; it never overwrites an existing file. `key import`
stores a private JWK in the tenant's local vault, refuses to replace an existing
key unless `--force` is supplied, and warns when the imported `kid` is not in
the default issuer's published key set. `key list` displays the default issuer's
public key attribution and marks the key whose private half is in this vault;
`--json` prints only the published public-key array. `key remove` shows the
key's attribution and then requires `--force`, so a run without it previews
whose key you are about to revoke; it permits removing the last key. Removal is
**not verified to be immediate revocation** — see the open question in
`docs/api/17-jwt-bearer-user-tokens.md`. `key rotate` publishes a replacement
before storing it locally and removes the old public key afterward, so each
intermediate state retains a working key.

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
aic workspace init                            # scaffold the tenant tree (both realms + idm + typescript)
aic workspace update                          # refresh bundled types/config to the latest
```

Both commands also regenerate the tenant-derived types: ambient
`idm/types/managed/*.d.ts` for the `.cjs` scripts, and the module-form
`typescript/src/generated/managed.ts` for the TypeScript endpoint project.

Each names any **dangling reverse** it finds — a relationship whose
`reversePropertyName` points at a property the target object does not have. The
stock `alpha_application`/`bravo_application` objects ship with six. The reverse
side is missing from the runtime too, so the omission in the generated types is
correct; the warning exists so a member you expected and cannot find reads as a
tenant schema defect rather than a generation bug
(`docs/api/10-managed-objects.md`).

`update` refreshes every managed file and **adds the TypeScript project to a
workspace that predates it**, seeding its example endpoints. A seed you have
not edited is refreshed when the template moves; a seed you have edited, or
one whose origin cannot be verified (no recorded hash), is left alone and
named in the output. Deleted seeds stay deleted. `typescript/package.json`
is merged rather than replaced — the framework's toolchain entries are
refreshed, any dependency you added is kept.

`update` also removes **generated-only** script folders: a leaf
`tsconfig.json` (and, for AM libraries, the `export * from "./….cjs"`
wrapper) left behind when a script was deleted and recreated under a
different context, or when the `.cjs` was later removed by hand. A folder
that still contains a script source, or any file this tool did not generate,
is left alone. Safe to re-run; a workspace with no orphans is a no-op.
`aic script delete` still keeps the local `.cjs` — that is the user's
source, not scaffolding.

### The TypeScript endpoint project

`workspace/<tenant>/typescript/` lets you write custom endpoints as ordinary
TypeScript modules with typed routing and validation, and bundles each one into
a self-contained ES5 file in `idm/endpoint/`. IDM has no module system, so this
is the only way to share code between two endpoints without an `openidm.action`
hop. Full design: `docs/typescript-endpoints.md`.

```bash
cd workspace/<tenant>/typescript
npm install
npm run check          # type-check + lint + test + build
npm run watch          # rebuild on save — pair with `aic script watch`
```

Needs **Node 22.18+ or 23.6+** (declared in the project's `engines`): `npm test`
runs the `.ts` test files through `node --test` directly, which relies on native
type stripping being on by default.

The build writes `idm/endpoint/<name>.cjs`, an OpenAPI 3.1 document per endpoint
under `typescript/openapi/`, and an ownership manifest that `aic script watch`
reads (below).

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
aic script list [<ref>] [--context TEXT] [--default | --no-default] [--json]
aic script create <ref> --context <ctx> [--from FILE] [--language LANG] [--evaluator-version V] [--description TEXT] [--tenant TENANT] [--yes] [--no-syntax-check]
aic script copy <src-ref> <dst-ref> [--tenant TENANT] [--yes] [--no-syntax-check]
aic script delete <ref> --force [--tenant TENANT] [--yes]
aic script pull [<ref>] [--force]               # pull; no ref → fuzzy picker
aic script push [<ref>] [--force] [--yes] [--no-syntax-check]  # push edits; --force makes tracked tenant scripts match local
aic script sync [<ref>] [--resolve local|remote] [--tenant TENANT] [--yes] [--no-syntax-check]   # reconcile, or make one side win for every selected entry
aic script watch [--tenant TENANT] [--yes] [--no-syntax-check]   # auto-push each .cjs you save (Ctrl-C to stop; also creates generated endpoints)
aic script status [<ref>]                       # in sync / modified / remote / conflict; template/type drift notes
aic script diff [<ref>] [--local-vs-snapshot | --snapshot-vs-remote]
aic script who <ref> [--history] [--minutes N] [--json]   # who created/last modified it
```

- `list` tags each row with its `ref` and narrows three ways.
  `--context TEXT` keeps AM scripts whose context **or** workspace folder slug
  contains `TEXT`, case-insensitively — so `--context OAUTH2_VALIDATE_SCOPE`
  returns both the legacy context and its `_NEXT_GEN` sibling, and
  `--context decision-node` returns both engine generations while
  `--context decision-node-legacy` returns only the 1.0 ones. It matches no IDM
  script, because only AM scripts have a context. When the filter keeps nothing,
  the contexts the tenant actually has are printed on stderr.
- `--default` / `--no-default` split the listing on the **DEFAULT** column,
  which means _shipped with the product_ (AM's `default: true`) — the scripts
  `script delete` refuses to remove. It does **not** mean "the script this realm
  is configured to run". That question is answered by
  `aic oauth provider get` (realm-wide, e.g. `validateScopeScript`) and by the
  client's own `overrideOAuth2ClientConfig` (per-client). Both report a script
  UUID; resolve it against the `ID` column of a listing.
- `create`, `copy`, and `delete` apply only to standalone AM scripts, IDM
  endpoints, and IDM schedules. Managed hooks and sync-mapping scripts are slots
  in their owning configuration documents.
- AM `create` requires `--context`; it accepts either an AM context constant or
  the workspace folder slug (such as `decision-node` or `lib`). `copy` is
  same-tenant only (including alpha-to-bravo cross-realm copies), retains the
  complete source config, and both create/copy pull the server's canonical form
  into the workspace.
- `create` refuses legacy (`evaluatorVersion: "1.0"`) scripts, and it asks the
  tenant which engines a context supports **before** writing anything. It has
  to: the scripts endpoint accepts a legacy-only context with
  `"evaluatorVersion": "2.0"` and stores `1.0` anyway — `201`, no warning — so
  the refusal was previously unreachable for exactly the contexts it exists
  for, and you found out at runtime. Now `--context OAUTH2_VALIDATE_SCOPE`
  creates under `OAUTH2_VALIDATE_SCOPE_NEXT_GEN` and says so on stderr. Where a
  context has no next-gen form at all — `AUTHENTICATION_SERVER_SIDE`,
  `AUTHENTICATION_CLIENT_SIDE` — it refuses and names what the context does
  support. Do not infer the engine from the context's name or its language
  list. The global context list says `SAML2_SP_ADAPTER` is `JAVASCRIPT` only,
  while the realm's own `contexts/SAML2_SP_ADAPTER` reports
  `evaluatorVersions: {JAVASCRIPT: ["1.0"], GROOVY: ["1.0"]}` — the two AM
  endpoints disagree, and only the second one answers the question being asked
  (measured 2026-09-10). That is why this is a live call and not a table. The
  check is keyed on the language the create will send, so `--language GROOVY`
  refuses everywhere a next-gen engine is required — every next-gen context
  advertises JavaScript alone.
- `watch` normally pushes only **tracked** scripts, and silently skips an
  untracked file. The one exception is an endpoint the TypeScript project
  declares it owns in `typescript/.aic-ts-manifest.json`: that has no snapshot
  precisely because it has never existed remotely, so watch **creates** it on
  the tenant (honouring the same prod guard as a push) and every later save
  takes the ordinary tracked path. Hand-written `.cjs` files are unaffected.
  If the tenant already has that name — a bundle built in another checkout, or a
  create whose pull-back never finished — watch **adopts** the tenant's copy as
  the baseline instead, writing nothing to the tenant and backing that copy up
  when it differs from the file on disk. The next line of output is the ordinary
  conflict-aware push. Adopting is the step that was missing: `create` refuses a
  taken name and `push` refuses an unsynced one, so before this every save
  repeated the same refusal and no suggested command could clear it.
- Ctrl-C stops `watch` from anywhere, including at a conflict prompt. The prompt
  runs in raw mode, where the terminal raises no SIGINT, so the interrupt
  reaches the process only as a keypress; it used to read as "skip" and leave
  the watcher running. `sync` treats it the same way — Ctrl-C ends the run, Esc
  still skips the one conflict.
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
  push is blocked and a 3-way diff is shown. `--force` means make each selected
  tracked tenant script match its local source, even when local equals the
  snapshot; `push all --force` therefore checks every tracked script. It does
  not create or adopt untracked scripts. When remote already equals local, no
  PUT is sent and the snapshot is refreshed from the live resource.
- **Accepted script writes are confirmed before the snapshot advances.** Push
  and sync re-fetch the resource and compare its decoded source with the exact
  bytes submitted. A mismatch reports “write accepted, but read-back did not
  match” and a failed fetch/decode reports “confirmation failed”; both leave
  the local source and snapshot unchanged, describe the tenant state as
  uncertain, and exit non-zero.
- **Explicit sync resolution applies to every selected entry.** `--resolve
  local` makes the tenant match each existing local source via the same forced,
  syntax-checked, confirmed push; a missing local file is an error and is not
  restored. `--resolve remote` makes every local source match the tenant,
  backing up differing existing source first. Without `--resolve`, sync keeps
  its three-way behavior: local-only changes push, remote-only changes pull,
  equal changes converge, and genuine conflicts prompt when a terminal is
  available.
- **Pull backups are based on the bytes being replaced, not the snapshot.** A
  normal `pull`, a reconcile pull, and `sync --resolve remote` write any
  differing existing source to `.aic-sync/backups/` before replacing it, even
  if that source equals the snapshot. Backup names include kind, realm, and
  script name plus a collision-safe suffix; the actual path is printed. A
  backup failure aborts before local source or snapshot changes. Direct
  `script pull --force` remains the explicit opt-out; sync never opts out.
- **Every write is syntax-checked first.** Before `create`, `copy`, `push`,
  `sync` and `watch` write anything, the tenant is asked to parse the source —
  AM through `scripts?_action=validate`, IDM through `script?_action=compile`.
  Neither write path does this itself: a `PUT` of a script that does not parse
  succeeds with a 201, and the breakage then surfaces far from the edit. A
  broken AM script fails whenever its journey or token flow next evaluates; a
  broken IDM endpoint answers **404** at its runtime URL while its config
  object still reads back 200, so it presents as an endpoint that was never
  created.

  A refusal writes nothing, leaves the snapshot alone, and exits non-zero, so
  the local edit survives for you to fix and push again. That includes a batch:
  `push all` and `sync` finish the run and print their summary, then exit
  non-zero if anything was refused. AM reports the line and column; **IDM
  reports neither for JavaScript** — its message is the bare parser string
  ("syntax error") even when the fault is on line 40 — and the output says so
  rather than leave you hunting for a coordinate that was never sent.

  **No verdict is also a refusal, with no exceptions.** If the check cannot
  answer — an unexpected body, or a 503 from IDM's compile action, which is
  what it returns for a script `type` it does not recognise _and_ what an
  unwell service returns — nothing is written, and the message says the check
  gave no verdict rather than that the source was rejected. Retrying is the
  first thing to try, since it may be a bad minute on the tenant.

  The same goes for source nothing _can_ check, such as an endpoint declaring
  a script engine the compile action does not compile: that is recognised
  before the call is made, so the message names the type and skips the useless
  retry advice — but it still writes nothing, because unparsed source is
  unparsed source however the gate found out. `--no-syntax-check` is the one
  way to store source the tenant has not parsed.

  `--force` does **not** override this, and that is deliberate: drift is a
  question of whose content wins, while an unparseable script is broken
  whoever wrote it. `--no-syntax-check` is the escape hatch, on every one of
  those commands, for when the check itself is in the way. The pre-flight is
  one extra call, ~0.15s.
- **`status` filters.** `am`/`idm` are group aliases; anything else is a
  case-insensitive substring of the full-name (use a trailing slash, e.g.
  `alpha/`, to match only that AM realm and exclude `managed/alpha_user…`).
- **`status` reports workspace drift** after the script rows. It prints the
  same templates-version nudge that `push`/`pull` already print when the
  scaffold predates the bundled templates. Independently, it fetches the live
  managed schema and regenerates the expected type files in memory (nothing
  is written): if any on-disk managed type file is missing or differs, one
  line names how many are stale and points at `aic workspace update`. Both
  notes stay silent when current. A failed fetch warns once and does not fail
  the command — the script rows still printed.
- **`diff`** shells out to `git diff --no-index` (needs `git` on PATH): colored
  via your pager interactively, plain unified diff when piped
  (`aic script diff bravo/Foo | delta`). Default compares local vs tenant;
  `--local-vs-snapshot` shows your edits since the last pull,
  `--snapshot-vs-remote` shows tenant drift since you pulled.

- **`who`** answers "who last touched this, and when?" — the recurring question
  when a script changed and nobody remembers doing it. It resolves AM's
  principal DNs (`id=<uuid>,ou=user,ou=am-config`) to names, so the output reads
  `by David Balmain` rather than a DN.

  Four answers are honest rather than failures, and each is worded distinctly:

  - **`unknown (AM recorded no author)`** — AM stores the _string_ `"null"` for
    scripts it shipped or imported. Over half the scripts on a mature tenant
    look like this, and the author being unknown says nothing about the date,
    which is often present.
  - **`service account "<name>"`** — including every write `aic` itself makes. A
    follow-up line says so explicitly: a service account is a shared credential,
    so it identifies the credential and never which operator used it.
  - **`dsameuser (AM-internal account — not readable)`** — AM's own principal;
    the lookup is refused with 403 by design.
  - **`<id> (deleted principal)`** — the account that made the change is gone.

  **Only AM scripts record authorship at all.** IDM config objects (`endpoint/`,
  `schedule/`, `managed/`, `sync/`) store neither an author nor a revision, so
  `who` says so and points at the logs instead of guessing.

- **`who --history`** lists earlier writers from the `am-access` logs, since the
  fields only ever name the _latest_ one. It needs log API keys (see
  `aic logs`). `--minutes` defaults to 60 and is capped at **1440 — a server
  limit, not ours**: the log API rejects any query spanning more than a day.
  Events are retained about 30 days, so anything older is still there but needs
  the window placed further back rather than widened.

> After upgrading the binary, restart the agent (`aic session stop` then
> `aic session login`) so it loads new `Accept-API-Version` headers used by
> AM-script support.

---

## See also

- [README](../README.md) — what the tool is, setup, and the agent model.
- [`docs/api/`](api/) — verified AIC endpoint reference (read before changing
  code that hits a tenant).
