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
The command refuses production-themed tenants and requires a key from aic
jwt-bearer setup.

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

## `aic role` — IDM internal roles

Internal roles are tenant-global. Their `_id`, rather than their display `name`,
is what IDM authorization configuration references. Creating through this
command makes the caller-chosen id the default name as well.

```bash
aic role list [--json]
aic role show <id> [--json]
aic role create <id> [--name <name>] [--description <text>]
aic role delete <id> [--force]
aic role privilege list <role-id> [--json]
aic role privilege add <role-id> --path managed/alpha_user --permissions VIEW,UPDATE --attr mail:rw --attr userName:ro [--privilege-name <name>] [--actions action1,action2]
aic role privilege rm <role-id> --path managed/alpha_user
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
`--yes`, `--dry-run`, and `--no-backup`. `list` prints the whole-document digest
used by `--if-digest`; a write with a stale digest is refused. It also prints
each rule's 0-based index and 8-character rule digest. Writes use the index
because duplicate rules are legal—several byte-identical entries make “replace
this entry” ambiguous by content. `show` may use either address; a digest that
identifies duplicates shows every matching entry, and `list --duplicates`
filters to all members of duplicate groups.

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
`.aic/backups/access-<tenant>-<UTC>.json` unless `--no-backup` is supplied.
`--dry-run` prints the rule-level change summary without writing or creating a
backup. Writes prompt after showing the summary unless `--yes` is supplied;
global `--no-prompt` therefore requires `--yes` for a real write.

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
`--no-backup` equivalent, so a failed backup blocks the write there too.

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

> `*-encrypted` fields are cluster-local and stripped from every client PUT;
> server-managed metadata is also removed and `_rev` is ignored (plain PUT). See
> `docs/api/05-oauth2-oidc.md`.

## `aic jwt-bearer` — Trusted JWT Issuer setup

```bash
aic jwt-bearer setup [--realm alpha] [--tenant NAME]
aic jwt-bearer issuer create <id> --issuer ISS --jwks-from FILE [--realm alpha] [--tenant NAME]
aic jwt-bearer issuer show [<id>] [--realm alpha] [--tenant NAME]
aic jwt-bearer key list [--realm alpha] [--tenant NAME] [--json]
aic jwt-bearer key remove <KID> --force [--realm alpha] [--tenant NAME]
aic jwt-bearer key rotate [--realm alpha] [--tenant NAME]
aic jwt-bearer key export [--tenant NAME] [--out FILE]
aic jwt-bearer key import <FILE> [--realm alpha] [--tenant NAME] [--force]
```

`setup` creates or updates the default lower-environment issuer, merges this
install's public key into its shared key set, and stores the private key in the
per-tenant encrypted vault. It is idempotent. All JWT-bearer writes are refused
on production-themed tenants; no confirmation flag overrides that refusal.
`issuer create` imports an existing public JWKS under a named issuer, and
`issuer show` prints one issuer or the realm's issuer list as JSON. `key export`
writes the tenant's private signing JWK either to stdout or to a new mode-600
`.jwk` file; it never overwrites an existing file. `key import` stores a private
JWK in the tenant's local vault, refuses to replace an existing key unless
`--force` is supplied, and warns when the imported `kid` is not in the default
issuer's published key set. `key list` displays the default issuer's public key
attribution and marks the key whose private half is in this vault; `--json`
prints only the published public-key array. `key remove` shows the key's
attribution and then requires `--force`, so a run without it previews whose key
you are about to revoke; it permits removing the last key. Removal is **not
verified to be immediate revocation** — see the open question in
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

`update` refreshes every managed file and **adds the TypeScript project to a
workspace that predates it**, seeding its example endpoints once. It never
overwrites your own endpoints or shared modules, and `typescript/package.json`
is merged rather than replaced — the framework's toolchain entries are
refreshed, any dependency you added is kept.

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
aic script list [<ref>] [--json]                # list scripts (each row tagged with its `ref`)
aic script create <ref> --context <ctx> [--from FILE] [--language LANG] [--evaluator-version V] [--description TEXT] [--tenant TENANT] [--yes]
aic script copy <src-ref> <dst-ref> [--tenant TENANT] [--yes]
aic script delete <ref> --force [--tenant TENANT] [--yes]
aic script pull [<ref>] [--force]               # pull; no ref → fuzzy picker
aic script push [<ref>] [--force] [--yes]       # push local edits; no ref → fuzzy picker
aic script sync [<ref>] [--resolve local|remote] # reconcile: push local-only, pull remote-only
aic script watch                                # auto-push each .cjs you save (Ctrl-C to stop; also creates generated endpoints)
aic script status [<ref>]                       # in sync / modified / remote / conflict
aic script diff [<ref>] [--local-vs-snapshot | --snapshot-vs-remote]
aic script who <ref> [--history] [--minutes N] [--json]   # who created/last modified it
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
- `watch` normally pushes only **tracked** scripts, and silently skips an
  untracked file. The one exception is an endpoint the TypeScript project
  declares it owns in `typescript/.aic-ts-manifest.json`: that has no snapshot
  precisely because it has never existed remotely, so watch **creates** it on
  the tenant (honouring the same prod guard as a push) and every later save
  takes the ordinary tracked path. Hand-written `.cjs` files are unaffected.
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
