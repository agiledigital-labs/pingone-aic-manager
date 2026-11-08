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
- **`--tenant <name>`** overrides everything for a single call. Without it the
  tenant comes from, in order: a `workspace/<tenant>/` directory you are inside
  (only if that tenant is configured), the stored context (`aic ctx use`), then
  `default_tenant` in `.aic/config.toml`. `aic ctx current` prints the tenant
  that wins here and which rule chose it. Every command with a `--tenant` flag
  (all but `ctx`) prints the same on stderr before anything else — before the
  unlock prompt, so you see which tenant you are unlocking for, and before a
  no-op, so "nothing to do" names its tenant too:

  ```text
  target tenant: dev (current directory is in workspace/dev/)
  ```

  The tenant is resolved once, when the command line is parsed, and everything
  in that invocation uses that answer; if the stored context cannot be read
  at that point, every command needing the implicit tenant fails with that
  error rather than retrying. Commands whose output is machine-read print no
  banner, so their stdout and stderr stay clean for `$(…)`: `aic whoami
  --token`, `aic auth --token`, `aic jwt-bearer key list --json`,
  `aic jwt-bearer key export` without `--out`, and
  `aic jwt-bearer subjects list --json`. Bare `aic` (the TUI) opens on the
  same tenant.
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

  The complete grammar is `--force`/`--force=operation` for the command's
  primary override, `--force=syntax-check` for script syntax bypass, and
  `--force=backup` for backup bypass. Optional values never consume the next
  argument: use `=`, not `--force backup`. Repeat the flag to grant independent
  permissions, for example `--force --force=backup`. Unsupported guard names are
  parse errors before execution, including on preview commands.

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

  One exception to "stands in for the cwd": when `--project` names a directory
  with no tenant of its own (typically the project root) and your real cwd is
  inside `workspace/<tenant>/` **of the same project**, the cwd's tenant is
  used. Before this, an editor task passing `--project <root>` from inside
  `workspace/dev/` silently targeted the stored context instead. A `--project`
  inside `workspace/<tenant>/` still wins over the cwd, and a cwd in a different
  project is never consulted. A task that relied on the old behaviour to reach
  the stored context from inside another tenant's workspace should pass
  `--tenant`.

  `--project` is `global = true`, so it is accepted before or after the
  subcommand (`aic --project ~/w/x esv list` and `aic esv list --project ~/w/x`
  are the same). It is resolved from raw argv before parsing, because the
  process has to be rooted before `--tenant`'s default can be read out of the
  project's own config.

  **Relative file paths** given to any flag or argument (`--from`, `--file`,
  `--out`, `--key-file`, …) resolve against the same start directory: your cwd,
  or the `--project` / `AIC_PROJECT` directory when one is given. Before this
  they resolved against the project root, because `aic` moves into the root
  before parsing, so `--from idm/endpoint/x.cjs` typed inside
  `workspace/sandbox/` looked for `<root>/idm/endpoint/x.cjs`. A script that
  passed root-relative paths from a subdirectory should pass them relative to
  where it runs, or absolute. Absolute paths are unchanged.

- **Non-interactive mode.** Pass the global `--no-prompt` flag, or set
  `AIC_NO_PROMPT=1`, to disable every interactive prompt. If input is required,
  the command fails instead of waiting on a terminal. Confirming a missing
  operator name is optional: non-interactive commands use the best fallback for
  that run and leave the setting unset for a later real terminal.

### Flag migration

Confirmation flags now have one meaning each. Old spellings were **removed**,
not kept as hidden aliases — using one is a parse error. `--yes` help is
uniformly "Confirm a write to a production-themed tenant."

| Old                                                                     | New                                                             |
| ----------------------------------------------------------------------- | --------------------------------------------------------------- |
| `script create\|copy\|push\|sync\|watch --no-syntax-check`              | `--force=syntax-check`                                          |
| `script pull --force` (skipped the overwrite prompt **and** the backup) | `--force` (prompt only) **and** `--force=backup` (backup only)  |
| `script sync --resolve local\|remote`                                   | `script sync --resolve local\|remote --force`                   |
| `access … --no-backup`                                                  | `--force=backup`                                                |
| `access … --yes` as the unattended summary accept                       | `--force` for the summary; keep `--yes` only for production     |
| `managed field edit --allow-narrowing`                                  | `managed field edit --force`                                    |
| `esv secret destroy\|delete --yes` (prod **and** the typed `yes`)       | `--yes` (prod) **plus** `--force` (skip the typed confirmation) |
| `ctx rm --delete-keys`                                                  | `ctx rm --purge all --force` (plus `--yes` on production)       |

The same protected-pull split as `script pull` now also applies to `oauth pull`,
`journey pull`, `policy pull`, `policy set pull`, and `policy rt pull`: those
four resource kinds did not previously back up or prompt before overwriting a
local edit. Bare `--force` authorizes the overwrite and still writes a backup
under `.aic-sync/backups/`; `--force=backup` skips only the backup and never
authorizes the overwrite.

`script create`, `copy`, and `watch` accept `--force=syntax-check` only and
reject bare `--force`. `push` and `sync` combine both permissions:

```bash
# old
aic script push endpoint/x --no-syntax-check
# new
aic script push endpoint/x --force=syntax-check

# old: skipped both overwrite consent and backup
aic script pull endpoint/x --force
# new: spell both permissions to preserve that exact behavior
aic script pull endpoint/x --force --force=backup

# old
aic script sync --resolve remote
# new
aic script sync --resolve remote --force
```

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
| `aic ctx list [--json] [--domains]` | List tenants in `.aic/config.toml`, marking the effective tenant and the stored context. See below.            |
| `aic ctx current [--tenant]`   | Print the effective tenant (stdout) and why it won (stderr). Alias: `aic ctx show`. See below.                 |
| `aic ctx use <tenant>`         | Switch the active context.                                                                                     |
| `aic ctx scopes [--tenant] [--add S]` | Show the scopes the tenant grants this install's SA beside the ones its tokens request; `--add` requests an already-granted scope. The SA cannot grant itself one. |
| `aic ctx rm <tenant>`          | Remove a tenant entry and the local artifacts belonging to it. See below.                                      |
| `aic whoami [--tenant <name>]` | Mint and print token info plus the local operator name and host for a context.                                 |
| `aic whoami --token`           | Print **only** the bearer token (for scripting, e.g. `curl -H "Authorization: Bearer $(aic whoami --token)"`). |
| `aic whoami --token --host <url-or-host>` | As `--token`, for the context whose host the URL names (unless `--tenant` is given); refuses — no token printed, non-zero exit — unless the host is that context's own or one of its custom domains. |

`aic ctx current` (or `aic ctx show`) answers "which tenant would a command
run here hit?", not "what did I last `ctx use`?". stdout is the bare tenant
name, so `$(aic ctx current)` keeps working; stderr says where it came from,
and names the stored context separately when it is not the one in effect, or a
`workspace/<name>/` directory skipped because no tenant has that name:

```text
$ cd workspace/dev && aic ctx current
dev
source: current directory is in workspace/dev/
stored context: sandbox (not in effect here)
```

With nothing configured at all it prints `<none>`. It used to print the stored
context regardless of where it was run, and `<default_tenant> (default)` when
there was none.

`aic ctx list` marks both: `EFFECTIVE` is the tenant a command run here would
hit, `STORED` the one `aic ctx use` last chose. The column used to be `CURRENT`
and meant the stored context. In `--json`, `effective` is new and `current`
keeps its old meaning (the stored context) for existing consumers.

Two contexts can share a word like "sandbox" in both name and base URL, which
is how a change once landed on the wrong one. The domain an operator types
into a browser tells them apart, so `whoami` prints the context's `host:` and
its `domains:` (custom domains), and `ctx list --domains` adds a
`CUSTOM_DOMAINS` column (`custom_domains` in `--json`). Both read
`GET /environment/custom-domains/{alpha,bravo}` from the tenant — `ctx list`
stays offline without the flag. That route needs `fr:idc:custom-domain:*`,
which onboarding does not request; without it the answer is
`unreadable — the token lacks fr:idc:custom-domain:*`, never `none`, because
a refusal says nothing about whether domains exist. Grant the scope to the
service account in the console, then `aic ctx scopes --add` it.

Once printed, a token leaves `aic`, so `aic` cannot see where it is sent. A
token sent to another tenant is rejected with a 401, which is confusing rather
than harmful. To catch the mistake before the request, pass the URL the
request will go to:

```sh
URL="https://<your-tenant>.forgeblocks.com/openidm/managed/alpha_user?_queryFilter=true&_pageSize=1"
curl -H "Authorization: Bearer $(aic whoami --token --host "$URL")" "$URL"
```

`--host` takes a URL or a bare host. Without `--tenant`, the URL chooses the
context: the one whose base URL names that host, so one command works for
every tenant `aic` holds. A host no context's base URL names (a custom domain,
say) is checked against the current context; two contexts on one host need
`--tenant`. The comparison is of the host only (case, port, userinfo and a
trailing dot are ignored), against the context's base URL and, only when that
differs, against the tenant's custom domains. It refuses, prints no token and exits non-zero when neither
matches, or when the custom domains cannot be read (the
`fr:idc:custom-domain:*` scope, above). With `curl` the refusal leaves an
empty bearer, so the request fails with a 401 rather than reaching the other
tenant with a valid token. `--host` does not help with a token already held
in a variable. Commands that both mint and send — `aic idm get`,
`scripts/verify-endpoint.sh` — pair the host with the token themselves.

The normal `whoami` output includes `operator: <name> on <host>`. When the name
has not been saved yet, the line says it is unset and points to
`aic settings set operator.name <name>`. `--token` remains exactly one bare
token on stdout.

`--token` also guarantees the token's remaining life. The agent's own floor is
60s, which is right for a request it is about to make itself and far too short
for a token handed to another process that will hold it — so `--token` asks for
at least **840s** and the agent mints a fresh one when the cached token is below
that. AIC issues 898s tokens, so in practice a scripted caller always gets more
than fourteen minutes, and the cached token is reused only within the first
minute of its life. Plain `whoami` deliberately sends no floor: its job is to
report the cache, and refreshing would make `expires:` say the same thing every
time.

Unlike the other `ctx` verbs, `rm` needs an unlocked agent: it reads the vault
to find out what the tenant owns, and withdraws a signing key from the tenant
itself.

#### `aic ctx rm` — remove a tenant

| Flag                          | Effect                                                                                      |
| ----------------------------- | ------------------------------------------------------------------------------------------- |
| `--dry-run`                   | Print the plan and exit, changing nothing.                                                  |
| `--json`                      | Print the plan as JSON and exit, changing nothing.                                          |
| `--purge all\|defaults\|none` | Select all offered artifacts, the TUI defaults, or no optional artifacts without prompting. |
| `--force`                     | Skip only the typed-tenant-name confirmation.                                               |
| `--yes`                       | Confirm a write to a production-themed tenant.                                              |

The command prints a plan first. Without `--purge`, it asks `[Y/n]` per
artifact—even when `--force` is present. `--purge defaults` uses exactly the
same default-on choices as the TUI, including provenance and implied-child
rules; `all` requests every offered artifact and `none` requests no optional
artifact. Every selection still passes through the planner and its sharing
guard. Unless `--force` is present, the command then requires the tenant name
typed back. Without a terminal, both an explicit `--purge` value and `--force`
are required; no selection default is assumed. Each row is one of four states:

- **offered**, defaulting to on — or to **off** when the credential's recorded
  provenance says you supplied it rather than `aic` minting it;
- **absent** — nothing stored, so no choice is offered;
- **refused** — a _surviving_ tenant entry still needs it. Neither `--purge` nor
  `--force` can override a refusal;
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

`--dry-run` and `--json` remain plan-only and take precedence over selection and
confirmation flags. A production-themed tenant still additionally requires
`--yes` for live execution. The removed `--delete-keys` spelling is
`--purge all --force`; see [Flag migration](#flag-migration).

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
aic esv status [--json]                         # pending changes, and whether a restart is running
aic esv apply [--yes] [--wait [--timeout <min>]] # restart the runtime to apply staged changes
```

`status` lists the variables and secrets the tenant reports as pending. Deletes
are never reported as pending, so a pending delete does not show there. A
pending list that says more pages follow is refused rather than under-counted.

`apply` is refused while a restart is already running. With `--wait` it reads
the restart state until `ready` follows a `restarting`, then reads the pending
lists afresh. It shows the elapsed time on one live line at a terminal and one
line per change elsewhere. Restarts measured at 3–4 minutes; `--timeout` (1 to
1440 minutes, default 15) bounds every read and pause while waiting, so a read that hangs
cannot outlast it. It exits non-zero if changes are still pending afterwards,
or if it gives up (the restart carries on). The API names no restart, so one
someone else starts between the pre-check and this `apply`'s request is
indistinguishable from it.

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

Destroying a version and deleting a secret require a typed `yes` confirmation on
an interactive terminal. Bare `--force` replaces that operation confirmation for
unattended use. Production independently requires `--yes`: neither flag implies
the other, so unattended production deletion needs both.

---

## `aic logs` — fetch, sync, search, compact

Logs use the tenant's separate API-key auth plane. `key create` mints a key pair
only while an admin-user session is available; the service-account bearer cannot
mint or read log keys. The resolved admin username names the remote credential,
but this standalone command does not set `operator.name`.

### Key management

| Command                                                                | What it does                                                                                                                                                               |
| ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aic logs key set [--tenant <name>] [--id <api_key_id>]`               | Store or replace the log API key pair in the vault. Prompts for the secret.                                                                                                |
| `aic logs key show [--tenant <name>]`                                  | Show whether a log API key pair is stored, and print the key id.                                                                                                           |
| `aic logs key rm [--tenant <name>]`                                    | Remove the stored log API key pair.                                                                                                                                        |
| `aic logs key create [--tenant <name>] [--cookie-name <name>] [--yes]` | Mint a new key pair from an admin session, then store it. Prompts for the AM session cookie value if needed; production requires `--yes` before any session input is read. |

### Remote fetch

| Command                                                                                                                          | What it does                                                                                     |
| -------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `aic logs sources [--tenant <name>] [--json] [--output <path>]`                                                                  | List available log source ids. `--json` prints the list as JSON; `--output` writes it to a file. |
| `aic logs tx <transaction_id> [--tenant <name>] [--source <csv>] [--output <path>] [--wait] [--timeout <secs>]`                  | Fetch all events for one transaction id. `--source` narrows to a comma-separated source list.    |
| `aic logs range <begin> <end> [--tenant <name>] [--source <csv>] [--query <crest>] [--output <path>]`                            | Fetch events in an ISO-8601 time range. `--query` adds an optional CREST filter.                 |
| `aic logs query <filter> [--begin <iso>] [--end <iso>] [--tenant <name>] [--source <csv>] [--output <path>]`                     | Run a CREST filter over the logs API. Defaults to the most recent 24 hours.                      |
| `aic logs grep <pattern> [--since <duration> \| --begin <iso> --end <iso>] [--source <csv>] [--limit <n>] [--max-time <duration>] [--tenant <name>] [--output <path>]` | Fetch live logs and match a case-sensitive substring within each normalized payload.             |
| `aic logs tail [--source <csv>] [--pattern <text>] [--tenant <name>]`                                                            | Follow live logs until Ctrl-C, optionally filtering on the same payload substring match.         |

`logs grep` defaults to `--since 15m`; durations accept positive whole seconds,
minutes, or hours (`30s`, `5m`, `1h`). `--begin` and `--end` are an alternative
fixed ISO-8601 window and must be supplied together. Matches form one JSON array
on stdout, or in `--output`; pages are filtered and written as they arrive
rather than accumulating the whole window in memory.

A grep reads every event in the window, and the log API serves it in
1,000-event pages no faster than one request per 1.05s, so a wide window is
slow: the default sources over the last hour on the sandbox were 395,112
events, read in 15 minutes (~440 events/s, measured 2026-10-31). Three ways to
bound it, each leaving the JSON array valid:

- `--limit <n>` stops after `n` matches. The result is a prefix of the full
  answer, so it exits **0**, and stderr says later events may also match.
- `--max-time <duration>` (same `30s`/`5m`/`1h` form) stops reading when the
  budget is spent and exits **non-zero**, naming how far the scan got. A
  script must not read "no more matches" from a range nobody finished reading.
- Ctrl-C stops the same way as `--max-time`: valid output, non-zero exit.

A scan running longer than ten seconds prints a progress line to stderr every
ten seconds — events scanned, the latest event time reached, matches so far.

`logs tail` begins with the most recent 15 seconds and then requests
**overlapping** windows until Ctrl-C. Each poll rewinds 90 seconds — never
before the moment the follow started — because the log pipeline lags ingestion
behind event time by tens of seconds, so contiguous windows silently drop any
event that arrives after its own window was already queried. Events already
printed are suppressed by the same identity the local store dedupes on, and that
set is forgotten once it falls behind the window, so a long follow stays
bounded. Each matched event is one compact JSON value on stdout (JSON Lines).
Startup, empty-poll/filter status, and the Ctrl-C acknowledgement go to stderr,
so redirecting stdout produces a clean event stream. There is no separate
polling-interval flag: all log requests already pass through the client's
1.05-second API throttle.

Both commands inspect `payload`, which the live API returns as either a raw
string (especially `idm-core`) or a JSON object. Objects are serialized to JSON
before the case-sensitive substring match; the original event shape is emitted.
Events are ordered by the documented top-level `timestamp` within each fetched
page before matching and emission.

Log ingestion can lag tens of seconds behind the request, so a short
`aic logs tx` result is not proof the script never ran. After every `tx` fetch
the CLI looks for the transaction's **completion event**, which depends on the
sources asked for:

| `--source` includes                                 | Completion event                          |
| --------------------------------------------------- | ----------------------------------------- |
| `am-access` or `am-everything` (the default does)   | `AM-ACCESS-OUTCOME`                       |
| otherwise `idm-access` or `idm-everything`          | IDM's single `access` event               |
| neither — e.g. `idm-core` or `am-core` alone        | none: an empty result cannot mean "done"  |

AM wins when both are present, because an AM request that calls IDM can log
IDM's `access` event before its own outcome. If the event is missing, a note
goes to **stderr** (stdout stays the JSON event list, including when redirected
or written with `--output`):

```text
note: no AM-ACCESS-OUTCOME event yet for transaction <id> — logs can lag
tens of seconds behind the request; retry, or pass --wait
```

For sources with no completion event the note says so and suggests adding an
access source or reading a narrow window with `aic logs range`, instead of
advice to wait for an event those sources never write.

`--wait` polls the same transaction query until the completion event arrives
or `--timeout` seconds elapse (default 60). Progress lines go to stderr.
Timeout does not fail the command: the events from the last poll are still
written. `--timeout` is accepted without `--wait` and ignored, and sources with
no completion event are not polled at all. Range and query have no equivalent
completion signal, so they have no `--wait`.

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
aic managed get alpha_user [--fields GLOB] [--hook-sources] [--schema-only | --property NAME]
aic managed object create custom_widget [--title T] [--description D] [--yes] [--json]
aic managed object rename custom_widget custom_gadget [--yes] [--json]
aic managed object delete custom_gadget [--yes] [--json]
aic managed field add custom_widget.code --type string [--title T] [--required false] [--enum value[:Title] ...] [--default VALUE] [--yes] [--json]
aic managed field edit custom_widget.code --searchable true [--enum value[:Title] ... | --clear-enum] [--default VALUE | --clear-default] [--force] [--yes] [--json]
aic managed field rename custom_widget.code external_code [--yes] [--json]
aic managed field delete custom_widget.external_code [--yes] [--json]
aic managed hook add custom_widget onCreate [--yes] [--json]
aic managed relationship set custom_widget.owner --target alpha_user --forward one [--reverse none|one|many] [--reverse-key widgets] [--check] [--yes] [--json]
aic managed relationship delete custom_widget.owner [--remove-mapping [--force]] [--yes] [--json]
aic managed relationship verify custom_widget[.owner] [--sample 5] [--scan 200] [--probe [--force] [--yes]] [--json]
aic managed relationship relink custom_widget.owner [--dry-run] [--force] [--yes] [--json]
aic managed relationship map custom_widget.owner [--capture FILE] [--acknowledge-request-finished] [--dry-run] [--force] [--yes] [--json]
aic managed relationship capture-links --lower dev --upper uat --output FILE [--new-pair object.key]... [--dry-run] [--force] [--yes] [--json]
aic managed relationship restore-links FILE [--tenant uat] [--acknowledge-request-finished] [--dry-run] [--force] [--yes] [--json]
aic managed relationship rollback-map FILE [--tenant dev] [--acknowledge-request-finished] [--dry-run] [--force] [--yes] [--json]
aic managed relationship repair custom_widget.oldOwner [--dry-run] [--force] [--yes] [--json]
aic managed relationship snapshot custom_widget.owner [--output FILE]
aic managed relationship restore FILE [--dry-run] [--yes] [--json]
aic managed diff custom_widget repo/managed-objects/custom_widget/custom_widget.json [--strict] [--json]
```

`managed get` prints one object's definition as JSON. Inline hook `source`
bodies are omitted by default and replaced with the argument `aic script pull`
takes (`<script: managed/alpha_user.onUpdate>`), because those sources are
already first-class workspace scripts. Pass `--hook-sources` to include the real
source. `--fields` is a repeatable, both-ends-anchored glob over
`schema.properties` keys (`*` any run, `?` one character; case-sensitive);
`schema.order` and `schema.required` are narrowed to the surviving keys. If no
pattern matches, the command fails rather than printing an empty properties map.
A filtered document also notes on stderr how many properties are shown, and
names any pattern that matched nothing.

`--schema-only` prints just the `schema` member, still narrowed by `--fields`.
`--property NAME` prints one property's definition exactly as stored — the
shape to diff against a repository copy
(`diff <(aic managed get alpha_user --property mail) repo/mail.json`). The name
is exact and case-sensitive; a near miss names the property you probably
meant. It cannot be combined with `--fields` or `--schema-only`.

After the JSON, `managed get` prints each relationship's **storage** to stderr
— one line per pair, read from `config/repo.ds`: `generic`, `mapped
(reference / reverseReference)`, `MIXED` (one end mapped, the other not — the
measured broken state), `INCONSISTENT` for any other disagreement `verify`
reports, and `STALE` for a `repo.ds` mapping on a property `config/managed` no
longer declares. A pair in any of the last three names the `relationship
verify` command to run. stdout stays the definition alone, so the diff above is
unaffected. With `--property`, only that property's pair is shown. The TUI
relationship editor shows the same line under its title. What neither can show
is whether the running IDM has *picked up* a `repo.ds` change: `aic` knows of
no endpoint that reports it, so the line describes configuration, not runtime.

`managed diff <object> <file>` compares the tenant's definition of one object
with a repository file, read-only. The file holds that object's definition
(`{"name", "schema", …}`, as `managed get --hook-sources` prints it) or a whole
`config/managed` document, from which the named object is taken. Keys compare
as a set and arrays by position, since order is meaning in `schema.order` and a
`resourceCollection`. Each difference is printed as a JSON pointer with
`-` (tenant only), `+` (repo only) or `~` (changed). Two representation
differences are normalised away first, and each one applied is named on
stderr: `_refProperties._id`, which the console writes with
`propName`/`required` and Ping's shipped objects with a `description`, is
compared on `type` alone; and an event hook's body (`source` on the tenant,
perhaps `file` in a repository) is left to `aic script status`, while the
hook's presence, `type` and `globals` are still compared. `--strict` applies
neither. It exits non-zero when anything differs; `--json` prints
`{"object", "normalised", "differences"}`.

`--json` on a write prints what it changed, never the whole `config/managed`
document: `object create`/`object rename`, `field …` and `hook add` print the
one object they wrote; `relationship set`/`relationship delete` print
`{"changed": [objects]}`, every object whose definition changed (both ends of a
pair); `object delete` prints `{"deleted": "<name>", "changed": [objects]}`,
the second list being the objects whose inbound relationships it removed.
`object delete` also stops tracking the deleted object's hooks in script sync
(local files are kept), because their slots went with the object.

Every write accepts `--tenant <name>` and requires `--yes` for a
production-themed tenant. Field and relationship booleans take explicit values
(for example `--viewable false`). Field creation defaults to non-searchable,
viewable, user-editable, and optional; omitted field-edit flags leave that
attribute unchanged. On `relationship set`, an omitted `--reverse` likewise
**keeps** whatever the relationship already declares — including a reverse that
names a property the target object does not have — and `--reverse-key` defaults
to the declared name. It is `none` only when the relationship is being created.
Removing a reverse therefore takes an explicit `--reverse none`, which also
deletes the property from the target object. `<object>.<key>` must contain
exactly one dot. Every schema write is recorded for reversal from the TUI
history overlay; there is no CLI undo command.

Relationship keys cannot be renamed — not by `field rename`, and not by giving
`relationship set` a new `--reverse-key` for an existing reverse. (A new
forward key on `relationship set` is not a rename: it creates a second
relationship.) A rename of a mapped relationship was measured to strand its
existing links and leave the pair unable to read or write links from one side
(`docs/api/10-managed-objects.md`, "Relationship storage"); other renames are
unmeasured and refused to be safe. `aic` has no way to migrate links to a new
key, and delete-and-recreate is not a measured repair.

Every `config/managed` write — every verb here, the Managed tab, hook pushes
and TUI undo — is checked against `config/repo.ds` before it is sent, because
nothing here moves a `repo.ds` mapping with the property
(`docs/api/10-managed-objects.md`, "Relationship storage"). A write is refused when the resulting document would disagree with a
mapping where the current one does not: a pair with one end mapped and the
other not, a mapping naming a target its end no longer declares, a
`reverseReference` reading through a key that is not its partner's, both ends
holding the reference, a non-relationship property at a key mapped as a
relationship, or an object renamed away — by `object rename` or by undoing one —
while a mapping names it. A disagreement already present between the same ends
in every earlier read of the document does not block an unrelated write;
`relationship verify` reports it. The check reads `repo.ds` only when the write
adds a property, changes one's relationship shape, or renames an object,
compared with both the document the change was built from and a fresh read; if
a stale read (Q14) makes those two disagree, even a hook push reads `repo.ds`
and fails when it cannot. Two refusals come
earlier, with their own wording: `relationship set` retargeting an end when
either old end is mapped (measured to break a mapped end with no reverse;
refusing it for a mapped reverse is a precaution), and `object rename` of an
object a mapping names (inferred). `relationship delete` proceeds and notes on
stderr which mappings it leaves behind, which `relationship repair` removes; when `repo.ds` cannot be read for that
note it says so and deletes anyway. `object delete` prints
each inbound relationship it removed or narrowed.

`relationship delete` removes the property and, when it declares one, the
reverse property on the target, in one write. It refuses when the declared
reverse is not a relationship back to the source (use `field delete` for the
one property), when either end is a standard field, as `field delete` does,
and when either end targets more than one object type, because removing the
property would remove it for every target. `relationship set` holds a reverse
it drops or moves to the same rules.

For some seconds after any `config/managed` write, the objects it touched
flap: reads of the collection and of existing records — and writes to them —
answer `404 Resource 'managed/<object>…' not found`, then answer normally,
then 404 again. `relationship verify`, `relink` and the waits below retry that
404 for up to 30 s and say on stderr how long they waited, so a `verify` run
straight after a `delete` no longer fails on it. A record that does not exist
answers a different 404 and is never retried.

`relationship set` now creates mapped pairs: it confirms the `config/managed`
write first, then adds the named end's `reference` and its partner's
`reverseReference` in one `repo.ds` PUT. One-way relationships get only a
reference. Editing an existing relationship preserves its storage mode; no
`--generic` flag is offered. A failure before the mapping PUT leaves a
consistent generic pair (unless another writer changed it): finish with
`relationship map` before adding links. An uncertain PUT/read-back requires
`relationship verify` before retrying. Matching leftover mappings on a
recreated key are kept and checked for exposed orphaned values. With `--json`,
`mapping_verification` contains the read-only verification on create.
See [Relationship storage](relationship-storage.md) for storage, recovery,
column reuse and promotion implications.

A relationship end that a write declares is not live when the write returns:
a link written through it in the first second or so answers `200` and is
lost. `relationship set` therefore waits, after its write, until each end it
added answers a read through one of its object's records (up to 60 s), and
prints how long that took. When the object has no records, or the wait runs
out, it says so on stderr instead; the command still succeeds, because the
schema write has landed. `--json` adds `"activation"`, one entry per new end
with `outcome` `live`, `timeout`, `no_records` or `error`, and
`"activation_verified"`, true only when every probed end was `live`. `--check`
makes anything else exit non-zero — after the write, which has landed either
way — for a script that writes links straight afterwards. With `--check`
both ends of the relationship are probed, not only new ones, and a re-run
that has nothing to write still probes them: so re-running after a failed
`--check` on a type that still has no records fails again rather than
passing. No schema-level
wait exists for a type with no records: nothing measured so far tells a live
end from a pending one without a record to read through.

Adding a reverse to a relationship that already has links leaves every one of
them one-sided: IDM does not migrate them, so the new reverse end does not list
them. After such a `set`, `aic` checks up to 20 links from the first 200
records, warns when any is one-sided and names the command below; `--json` adds
`"one_sided": {"sampled", "one_sided"}`.

`relationship relink <object>.<key>` re-creates those links. It reads every
record of `<object>`, checks each link against the far end, and removes and
re-creates each link the far end does not list — a has-many link by deleting
its edge and creating it again, a has-one link by clearing the property and
setting it again. The new link has a new edge id and keeps its custom
`_refProperties`; whether hooks fire for these writes is unmeasured. A link the
far end lists under another edge id, or whose far record is gone, is reported
and left alone. `--dry-run` reports the plan and writes nothing. Otherwise it
asks for confirmation (`--force` skips it; with no terminal it requires
`--force`), then applies the production gate. Each link is re-read just before
its writes and skipped if it changed since the plan, and is confirmed from the
far end afterwards. The first failed write stops the run, and the error names
what was relinked; if the link was removed and could not be re-created, it
gives the request that restores it. It exits non-zero on a stopped run or on
any link it skipped as changed or could not confirm.

It refuses to start while the pair has a configuration finding from
`relationship verify` (one end mapped and the other not, say), because
re-creating a link cannot fix that. A link whose far end is a has-one already
holding another link is skipped: IDM refuses a link into an occupied has-one
(`400 … not available for assignment`), or on a generic has-one ↔ has-one
pair accepts it one-sided, and either would come after the removal.
Has-one writes carry `If-Match`, which IDM honours on a mapped end and ignores
on a generic one (`docs/api/10-managed-objects.md`, "Conditional link writes"),
so on a generic end the re-read just before each write is the only guard.

**Interrupted runs.** Each remove-and-create is recorded in
`.aic/managed-links.json` before the remove, and the path is printed before
the first write; the entry is cleared once a read shows the link back. The next
`relink` of the same field reads those entries first, judging each against
the field as `config/managed` declares it now. A link that is gone and not
replaced is re-created (`to_restore`, then `restored`, in `--json`; a dry run
lists `to_restore` only) and confirmed from both ends like any relink. One
that is back with its custom properties is forgotten. Everything else is named
for the operator, kept in the journal, and makes the run exit non-zero: a
has-one property now holding another link, a link back **without** the
properties it had (the journal holds the only copy of them), or a field whose
cardinality or targets changed since.

`relationship snapshot <object>.<key>` saves the relationship's definition —
and, when it declares a reverse, the reverse property too — as a JSON
document (`"format": "aic-relationship-snapshot/1"`) on stdout or to
`--output`. `relationship restore <file>` merges three things from it back into
each end it names: `title`, `description` (on the property, and on `items` for
a has-many) and each `resourceCollection` entry's `query.fields`, matched by
`path`. These are what recreating a relationship in the console was reported
to overwrite or trim. Nothing else is touched, so a snapshot older than a
structural change cannot undo the change, and a snapshot of a has-many cannot
be restored onto a has-one. It prints each change, `--dry-run` writes nothing,
and when the metadata already matches it says so and writes nothing.

`relationship verify` is read-only unless given `--probe`. For `<object>` it checks every
relationship on that object (each pair once); for `<object>.<key>`, one. A
relationship with several `managed/` targets is checked as one pair per target;
a target outside `config/managed` (`internal/role`, say) is listed as
unchecked, neither passed nor failed. It reads `config/managed` and
`config/repo.ds` and reports a pair whose two ends disagree — one end mapped in
`repo.ds` and the other not, a mapping that reads through a key or points at a
target the schema does not declare — plus any `repo.ds` relationship mapping
for a property the object no longer declares, or declares as something other
than a relationship. Being mapped or
generic is not itself a finding; both were measured to work. It then reads up
to `--scan` records from each end (one request per end) and checks up to
`--sample` of their links from the far end, which must list the near record
under the same collection, record id and edge id. An end with no links in the
records read is reported as untested, not healthy; a response that does not
decode is an error, never an empty sample. On each has-many end it sampled it
also tries the `/_ref eq` filter for one resolved link: a generic end is
measured not to answer it, so a miss there prints the limit and the filter to
use instead (`/_refResourceId`); a miss on a mapped end counts as a finding.
`--probe` covers a pair with no links yet: for each pair it
writes one test link from a record of the forward end to a record of the
reverse end, checks that the reverse end lists it, and removes it again,
confirming the removal by a re-read. It writes to existing records, so their
hooks fire; it asks for confirmation (`--force` skips it, and is required with
no terminal) and applies the production gate (`--yes`). Records are chosen so
the test link displaces nothing — through a has-one end only a record whose
property is empty — and the probe judges and removes only the edge it
created, identified by the edge id its write's response names
(`docs/api/10-managed-objects.md`), never an older or another writer's link
to the same record. When the response names no edge it removes nothing,
keeps the recovery entry and says so: a link that is merely new since the
probe's read may be another writer's. A pair with
no such record is reported
untested. A test link that resolves and is removed passes; one the far end
does not list, or that could not be removed, is a finding, and a leftover is
named with the request that removes it. Each probe is recorded in
`.aic/managed-links.json` before its write and cleared once the removal is
confirmed; the next `--probe` run reports any entry still there (`"recovery"`
in `--json`) and counts it as a finding. A has-one probe re-reads the
property before its cleanup and removes it only if it still holds the probe's
edge. A pair with an end its object lists in `schema.required` is not probed
and is reported untested: IDM's policy check refuses removing the test link
through a required end, so the probe could write its link but not clean it up.

The configuration lines describe what `config/repo.ds` **stores** ("stored
mapping … runtime unconfirmed"), which is not proof the running IDM has loaded
it. For one resolved link per direction, and for both ends of a resolved probe
link, `verify` also asks the running IDM with
`_queryFilter=_id eq "<id>" and <key> pr`, which was measured to find a linked
record on a mapped end and never on a generic one. Each such check prints a
`runtime:` line and appears in `--json` as `runtime` (on a direction) or as
`runtime: [{end, runtime}]` (on a probe): `mapped_confirmed` and
`generic_confirmed` agree with the stored config; `mapped_not_live` (stored
mapped, IDM answers as generic) and `live_not_stored` (the reverse) are
findings. A `pr` miss counts only while the record still holds the link, so a
miss is followed by a re-read that must still list the same edge; a link gone
or replaced by then (another writer) leaves the runtime unknown rather than
judged. An end with no resolved link is not
checked at runtime. The pair header labels each end `stored mapping, <kind>`
or `stored generic`, for the same reason.
`--sample 0` checks configuration only. It exits
non-zero on any finding or any link that does not resolve. A `STALE` finding —
a mapping naming no declared property — is what `relationship repair`
removes. For the other findings neither the console's repair nor
delete-and-recreate has been tested (`docs/api/10-managed-objects.md`,
"Checking a pair").

`relationship delete --remove-mapping` does the delete and then the repairs
its leftovers need, in one command. Before writing anything it plans every
removal against the document the delete will leave, in an order each
`repair` would allow on its own — so a `reverseReference` comes out before
the `reference` it reads through — and refuses outright, deleting nothing, if
any of them would be refused. It asks once (or takes `--force`), deletes,
then runs each removal exactly as `repair` does: re-read and recheck, backup,
whole-document replace, confirm by re-read. If one fails after the delete has
landed, it says what was deleted and unmapped, which removal failed (its own
error says whether it may have landed), and which were not attempted, and
points at `verify` to read what is mapped now before running `repair`. A
failed delete write itself says that no mapping was touched. `--json` adds
`"mappings_removed"`. Measured on the sandbox (2026-11-01) on a mapped
has-one pair: both entries removed in order, every other byte of `repo.ds`
unchanged.

`relationship map <object>.<key>` converts a generic pair to mapped storage:
**capture → switch → restore → verify**. The plan names the column, both
entries, missing per-type entries, link count and the window when links will
be invisible. Pause all link writers. Recreating links updates records: budget
for hooks, sync mappings and notifications per link; exact firing counts are
unmeasured. One-sided links in either direction and duplicate forward/target
edges refuse with a list, rather than silently changing their reports.

`--capture FILE` chooses the durable capture path and resumes an existing
in-place capture for that pair; otherwise a unique path under
`.aic/relationship-captures/` is printed. The file preserves forward IDs,
targets and custom `_refProperties` without `_id`/`_rev`. Full projected cursor
scans find generic links; `pr` does not. The capture is durably written before
any tenant write and never automatically deleted. Each request journals intent;
lost responses are reconciled from live data before further writes. Already
restored links are skipped, never duplicated deliberately. Captures bind both
tenant name and base URL. Keep their sidecars alongside them during recovery.

`map --dry-run` saves nothing and holds no permit. Otherwise it confirms
(or accepts `--force`, required without a terminal), passes the prod `--yes`
gate, repeats the census, rechecks declarations, backs up `repo.ds`, PUTs both
ends and any per-type entries in one document with `waitForCompletion=true`,
and confirms by re-read. Columns remain 1–10 per object, including
`explicitMapping` usage. Before restoring, `/<key> pr` must find no orphaned
column data. A finding leaves mapping and data intact and exits non-zero.

Restoration waits for captured relationship endpoints to be live, using set's
read-only activation wait, then uses the shared relink creation mechanics: has-many
POST, has-one conditional PATCH. Final verification compares the **entire**
captured set, including both ends' custom properties. Capture refuses
asymmetric reverse values before any write because preserving them is not yet
supported; differing per-side data can be legitimate and must not be equalised
to bypass this limitation. Verification rejects an extra
link or changed properties. Every link must resolve from the far end and the presence-query runtime
judgement must confirm mapped storage on both ends (forward only for one-way).
An empty pair remains runtime untested; no probe link is created.

`relationship capture-links --lower <source> --upper <destination> --output
FILE` prepares a promotion. It reads both tenants, selects declared lower
reference pairs mapped there and generic in the upper, and captures those
with upper links. Complete bindings (section, owner, column, target,
cardinality and reverse entries) are compared across all mapping sections,
including explicitMapping. Equal bindings skip only after comparing permissive
declaration footprints (presence, relationship/scalar, target set, cardinality,
reverse key), managed target-object presence, and schema reverse endpoints
even without mapping entries. Identical stock multi-target/internal shapes
and identically absent platform declarations skip without narrow conversion
validation; footprint differences refuse with both versions. Generic in both
also skips.
Different mapped bindings or an upper binding absent in lower refuse with both
versions: do not promote until resolved. Removing an upper binding is inferred
to hide its mapped links. No mapped-to-mapped migration is provided.
Missing declarations, including possible Q14 loss, refuse rather than proving
emptiness. For a mixed rollout, repeatable `--new-pair <lower-reference-end>`
explicitly acknowledges genuinely new upper pairs whose ends have never existed
there. Every bound and schema-named reverse end's upper declaration/mapping
must be absent; existing ends, stale mappings and typos refuse. Acknowledgment
precedes narrow validation, so genuinely new multi-target components can be
excluded without pretending their existing links could be migrated. Do not
use this to bypass possible Q14 loss.
Acknowledgments are printed and saved with any capture. The file also records
the complete lower and upper relationship bindings compared, including all
excluded pairs and every mapping section. It writes only a local capture,
never tenant config or records.
Even an empty selection gets a fresh declaration/binding recheck. A lower
custom declaration missing where upper still declares it names suspected Q14.
Equivalent bindings still refuse differing declaration footprints. Platform or
undeclared binding differences require investigation before promotion; repair
advice is limited to supported genericMapping cleanup, never explicitMapping.
An empty selection explicitly reports no file written. Restore only a file
this run confirmed writing; otherwise check empty/new pairs after promotion,
never reuse an older file at that path. The flags mirror the
`--lower`/`--upper` direction of `promotion verify`; this command has the same
confirmation/dry-run/prod flag shapes as map.

After the existing promotion workflow, `relationship restore-links FILE
--tenant <destination>` first re-reads the destination mappings and refuses
unless **every relationship binding**, including skipped pairs, matches the
saved lower snapshot. Any unexpected missing, extra or changed binding refuses
with expected/actual details and instructions to keep writers paused. It reports
all upper-before → upper-after binding changes; an addition alone proves
propagation, not replace versus merge. Do not bypass the upper-only refusal
to manufacture that measurement. Older promotion files without complete binding
snapshots refuse; re-capture only before promotion, otherwise retain the file
and escalate to the tenant administrator or Ping Support. It writes **records only**, never
config, then performs the same resumable restoration and exact verification
as map. Use the upper's own capture, not one from the lower environment.
A pending Create found present is verified and skipped automatically. If it
is absent, the request remains unresolved: restore refuses a duplicate send
and rollback refuses mutation. Pending Remove uses the same outcome rule:
absence confirms unlinking during rollback; presence refuses another unlink.
Default pending-Remove reversal refusal remains. New intents save their request's
`transaction_id` durably before dispatch and send it unchanged in
`x-forgerock-transactionid`; refusals print it. Look up that id with
`aic logs tx <id> --tenant <capture-tenant> --source idm-access --wait --timeout 120`.
IDM's terminal `access` event records completion and HTTP status for a measured
read and for a successful conversion link `PATCH` (measured 2026-11-01); a
failed write's event is unmeasured. An AM
introspection outcome is insufficient. Absence of a log is not proof the request
was never sent. Without terminal evidence retain the file, keep writers paused,
and contact the tenant administrator or Ping Support with the id. See the
[lookup and escalation procedure](relationship-storage.md). Older in-place
pending intents without an id require help identifying the original request.

`map`, `restore-links` and `rollback-map` accept
`--acknowledge-request-finished` only for an existing pending intent. Use it
only after establishing that the original request has finished and cannot
complete later; an absent read, elapsed time and --force are insufficient.
The assertion is scoped to the exact capture/intent, journaled with observed
presence and timestamp, and consumed before another request. It never grants
permission for a later lost response. Normal confirmation/prod gates and
schema/data guards still apply. Dry-run holds no permit and saves no audit.

The existing `relationship restore` restores definition metadata and is
separate. `--json` reports the capture path, link count and completion state.
Capture format version 2 requires both recorded custom-property views for
bidirectional links. Earlier development files refuse before tenant selection
with “captured by an earlier development build; re-capture”. No reverse values
are synthesised; capture original generic links before the switch, never after
they are hidden.

`relationship rollback-map FILE` is the explicit inverse for an **in-place**
capture only. Before any unlink, it plans full mapping removal (including
external reverse dependents) and checks every removal prerequisite. An
unchanged, backed-up/read-confirmed repo.ds PUT must be accepted; write denial stops before records are touched. Each
unlink also confirms mapped runtime to protect the hidden generic store.
It confirms removal of this conversion's mapped links before removing both
mapping entries in one guarded PUT, preserves per-type metadata,
then checks that the original generic set is visible again. Unknown or altered
data refuses. An interrupted rollback resumes with the same verb. While the
exact captured mappings remain installed, explicitly running restore-links
with a RollingBack capture instead restores removed links and reverses the
interrupted rollback only when no Remove intent is pending. An unresolved
Remove intent refuses direction change by default even when a link is visible;
finish rollback-map with the same file, keeping writers paused, or explicitly
acknowledge the original request is finished before reversing. No pending
request is retried automatically on the strength of the opposite observation. Once mappings are removed, finish rollback-map.
Promotion captures cannot authorize config writes; there is no upper-environment config
rollback here. A required forward property must be explicitly adjusted only
when mapped links actually need unlinking; an empty mapped store with no
unresolved request requires no schema change. No failure automatically rolls a conversion back.

Q14 can remove confirmed properties. Map re-reads both declarations immediately
before switching and again before restoration; restore and rollback recur
these guards during record work. Missing/changed ends stop with a Q14/schema
diagnostic. Preserve the capture, correct the tenant's schema problem and
retry. These reads cannot prevent a later tenant-side rollback.

The [operator runbook](relationship-storage.md) gives ordered dev conversion,
UAT/prod capture → promote → restore, resume and rollback commands. Promotion
carries repo.ds according to the operator's statement; records are never
promoted, and hidden upper-environment generic links follow by inference from
the measured switch. Source emptiness does not establish destination emptiness.

`relationship repair <object>.<key>` removes one `config/repo.ds`
relationship mapping (`reference` or `reverseReference`) that names a
property `config/managed` does not declare — the leftover a `relationship
delete` reports, which otherwise makes a later `set` on that key refuse. It
only removes; it never adds or edits a mapping. It refuses a declared
property, IDM's `_meta` / `_notifications`, a mapping another
`reverseReference` reads through — in any section of `resourceMapping` and on
any resource, `internal/…` and `explicitMapping` included (naming it; repair
a stale managed one first), the
last mapped property of an object, and an object `config/managed` does not
declare — the cases not measured. `--dry-run` prints the plan and writes
nothing; otherwise it asks, or takes `--force`, and passes the production
gate. Immediately before writing it re-reads both documents and refuses if
`repo.ds` changed at all. It backs the current document up to
`.aic/backups/repo.ds-<tenant>-<time>-<uuid>.json` and prints the path,
replaces the whole document — a JSON `PATCH` was measured to return 200 and
change nothing — and reads it back for up to 30 s. It exits zero only when a
read equals the document sent. Otherwise the outcome is judged over every
read, not the last: any read showing a third document is reported as another
writer, any failed read means "unchanged" was not observed, and a failed send
is never assumed not to have landed — each exits non-zero naming the backup.
It edits only `genericMapping."managed/<obj>"` and refuses a mapping found
only under another section such as `explicitMapping`, which `verify` and the
write guards read as well, or a key mapped in more than one section. There is no undo
entry: the backup is the way back (`docs/api/10-managed-objects.md`,
"Removing a stale mapping").

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
stderr even then. `field add` and `relationship set` reject `--force` because
narrowing does not apply to those commands. See
[Flag migration](#flag-migration) for the removed `--allow-narrowing` spelling.
Nothing fails at the moment you narrow: records holding a removed value still
read back, and patches to their other properties still succeed. What breaks is a
whole-record `PUT` of such a record — in some other integration, on a property
that code never touched. Adding a value, and `--clear-enum`, are both widening
and need no flag.

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

### `aic idm get` — raw read of any `/openidm/` path

```bash
aic idm get config/managed --fields objects/name
aic idm get "managed/alpha_user?_queryFilter=true" --fields _id,userName --all-pages
aic idm get /openidm/internal/role/openidm-admin
```

The path may be written `/openidm/<path>`, `openidm/<path>` or `<path>`, with
any query string. It is refused if it could leave `/openidm/` once the server
normalises it: a URL, a dot segment (encoded or not), an encoded slash or
backslash, an empty segment, a double-encoded or malformed escape, and any
character outside RFC 3986's path set (a space, tab, backslash or non-ASCII
character — percent-encode it). As a last check, the path the URL parser would
send must be exactly the path validated; one it would rewrite is refused.

- `--fields` is shorthand for `_fields=`; giving both is refused.
- Without `--all-pages` the response is printed as the tenant returned it, and
  a non-empty `pagedResultsCookie` adds a stderr note that more results follow.
- `--all-pages` follows `pagedResultsCookie` to the end and prints
  `{"resultCount": N, "pagesFetched": P, "duplicatesDropped": D, "result": [...]}`,
  with the same counts on stderr. A record whose `_id` an earlier page already
  returned is dropped and counted in `duplicatesDropped`. There is no tenant
  total to check `resultCount` against: the paging parameter is
  `_totalPagedResultsPolicy` (`_countPolicy` is a 400), and the tenant answers
  `EXACT` with an `ESTIMATE` that was measured wrong
  (`docs/api/10-managed-objects.md`, "Paging"). The walk is complete when it
  ends on an empty cookie, which is the only way it ends without an error. It adds `_pageSize=1000` unless the
  path sets one, and refuses a path that already carries
  `_pagedResultsCookie` or `_pagedResultsOffset`. A cookie returned twice stops
  the walk with an error rather than paging forever, and so does a
  `pagedResultsCookie` that is not a string. On a single resource it
  is refused, because the tenant rejects `_pageSize` there.

It only ever sends `GET`, but **GET is not a promise of no side effects**: a GET
on `endpoint/<name>` runs that endpoint's script.

---

## `aic journey` — authentication trees

Realm-scoped. Journeys pull/push as JSON exports (tree + all its nodes) under
the workspace.

```bash
aic journey list [--realm alpha] [--json]              # journey names
aic journey pull <name> [--realm alpha] [--force] [--force=backup] # protected local install
aic journey status [<name>...] [--realm alpha] [--check] [--json] # local / snapshot / tenant, read-only
aic journey diff <name> [--local-vs-snapshot | --snapshot-vs-remote] [--realm alpha] [--json]
aic journey push <name> [--realm alpha] [--force] [--yes] # push; production requires --yes
aic journey delete <name> --force [--realm alpha] [--yes] # delete; production requires --yes
aic journey using-script <script-uuid> [--realm alpha] [--json] # journeys referencing a script
aic journey nodes [--realm alpha] [--json]             # available node types
aic journey node-schema <nodeType> [--realm alpha]     # a node type's schema (JSON)
aic journey node-template <nodeType> [--realm alpha]   # a starter node config (JSON)
```

`pull` compares authored JSON content (ignoring recursive `_rev` fields) before
replacing the export. A missing export installs normally; an export equal to the
tenant or to its valid snapshot is safe. Any other existing export — including
malformed JSON, or one with a missing/malformed snapshot — is a protected local
edit and requires bare `--force` or an affirmative default-no terminal
confirmation. Every differing existing export is backed up under
`.aic-sync/backups/` first. Bare `--force` keeps that backup; `--force=backup`
skips only the backup and does not authorize the overwrite. Backup names include
journey/realm/name plus timestamp and UUID, and are mode 0600 on Unix. The
snapshot advances only after the export is installed successfully.

---

### `journey status` and `journey diff`

Both are read-only: they read the workspace export, the snapshot under
`journeys/<realm>/.snapshots/` and the tenant, and write nothing — the snapshot
is **not** refreshed, so asking cannot change what the next `push` decides.
They compare the way `push` does, ignoring `_rev` everywhere.

- `status` names each journey's state the way `script status` does: `in sync`,
  `modified locally`, `modified on remote`, `conflict`, `converged` (both sides
  made the same change; only the snapshot is stale), `no snapshot` (with
  whether local matches the tenant), `not pulled`, `not on the tenant`, and
  `not compared` when the tenant's copy could not be read whole — any node
  read failing with something other than 404. The row names the problem, and
  the command exits non-zero. A node that answers 404 is deleted on the
  tenant, and is compared as absent. A copy whose tree lists a node it does not
  carry is noted under its row (and in `notes` in `--json`) but still
  compared. With no
  names it covers every journey that has an export or a snapshot in the realm.
  Under each row it lists the parts that moved — `tree`, or a node by type, id
  and display name — on the `local` side (export vs snapshot) and the `remote`
  side (snapshot vs tenant).
- **Shared nodes.** A node's config is a realm object keyed by its id, so two
  journeys listing the same id share it, and pushing either rewrites it for
  both. When any node moved, `status` reads every tree in the realm once and
  marks such a node `shared with <journey>, …`. Nothing is read for that when
  only trees moved.
- `--check` exits non-zero unless every journey matches the tenant (`in sync`,
  `converged`, or `no snapshot` with local matching). `--json` prints
  `{ok, journeys: [{journey, realm, state, label, matches_tenant, problems,
  notes, local_changes, remote_changes}], sharing_unknown_for}`, each change carrying
  `shared_with`; `state` is `null` for a journey not compared, and
  `sharing_unknown_for` lists the trees that could not be read while looking
  for shared nodes, so a node shared only with one of them is not marked.
- `diff` notes on stderr a side whose tree lists a node it does not carry.
  It prints each difference as a JSON pointer under the part it belongs to,
  local against remote by default; `--local-vs-snapshot` shows your edits
  only, `--snapshot-vs-remote` remote drift only. It exits non-zero when the
  two differ.
- **Secret values are never printed.** A value whose key names a secret
  (`password`, `secret`, `token`, `credential`, `privateKey`, `apiKey`,
  `hmacSigningKey`, …, in any case and with `_`/`-` ignored, but
  not a `…Identifier` or `…Label` naming where one lives) shows as
  `"<hidden>"`, at its own path and inside any value printed whole. The change
  itself still counts. This is a name rule because the schema cannot be asked:
  AM's node schemas mark nothing secret — `product-CaptchaNode.secretKey` is a
  plain `"type": "string"` (read 2026-10-31).

## `aic policy` — AM policies, policy sets and the PDP

Realm-scoped, CLI-only (no TUI tab). Three collections — resource types, policy
sets (`applications` on the wire) and policies — plus `eval`, which asks the
policy decision point and then explains the answer.

```bash
aic policy list [--set <set>] [--realm alpha] [--json]
aic policy show <name> [--realm alpha]
aic policy pull <name> | --all [--set <set>] [--realm alpha] [--force] [--force=backup]
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

Pulls apply the inverse content rule before replacing local JSON. Missing local
files install normally; local content equal to the incoming tenant document or
to a valid snapshot is safe. A different local document with no trustworthy
snapshot is protected, as is malformed local JSON. A single named pull offers
one default-no confirmation; `--all` preflights every selected object and, if
bare `--force` is absent, refuses the whole batch while naming every protected
policy, set, or resource type before touching a file. Every differing existing
file is backed up with its collection/realm/name identity under
`.aic-sync/backups/`. `--force=backup` alone skips that recovery copy but never
authorizes an overwrite. Names include collection/realm/name plus timestamp and
UUID, and files are mode 0600 on Unix. The same behavior applies to
`policy set pull` and `policy rt pull`.

### `aic policy eval`

The reason this vertical exists. AM answers a denied request with `actions: {}`,
which means "no policy applied" and covers a resource that matched nothing, a
subject that failed, and a condition that failed — with no way to tell them
apart. `eval` reads the set, its resource types and its policies and says which
it was:

```console
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

## `aic promotion` — promotion lock state, status, reports and the write cycle

`status`, `reports`, `report` and `verify` only read; `lock`, `dry-run`,
`promote` and `unlock` write (below). Needs `fr:idc:promotion:*`
on the token (`aic ctx scopes`); a sandbox has no promotion service and answers
503. Run it against the **upper** environment of a pair — only that one has been
measured to report both sides' lock and to hold the reports. API: `docs/api/23-promotion.md`.

```sh
aic promotion status [--json]
aic promotion reports [--json]
aic promotion report [<report-id> | --provisional] [--json]
aic promotion verify --lower <env> --upper <env> [--report <report-id>] [--json]
aic promotion lock     [--tenant <upper>] [--force] [--yes] [--no-wait] [--timeout <minutes>]
aic promotion dry-run  [--tenant <upper>] [--force] [--yes] [--no-wait] [--timeout <minutes>]
aic promotion promote  [--tenant <upper>] [--force] [--yes] [--no-wait] [--timeout <minutes>]
aic promotion unlock   [--tenant <upper>] [--recover <promotion-id>] [--force] [--yes] [--no-wait] [--timeout <minutes>]
```

- `status` prints `lock/state` and the promotion status side by side. They have
  been measured to differ (`unlocked` against `UNLOCKING`); when they do, it
  names each difference and exits non-zero rather than pick one — with
  `--json` too, whose output carries `disagreements`, `blockingError` and
  `ok`. A reported blocking error exits non-zero on its own. Agreeing `LOCKED`
  values exit zero: it is not a readiness check.
- `report --provisional` is the promotion service's list of what it would
  promote, generated without taking a lock. It is **not** a live diff of the
  pair: on 2026-10-26 it listed 150 items as added or modified, and every one
  sampled was byte-identical on both tenants (`docs/api/23-promotion.md`). Output groups
  changes by section, item and `added`/`modified`/`deleted`, with each name and
  realm; a report whose shape it cannot count is refused, not half-summarised.
- 403 for scope, 503 for no service and 404 for no report each get a message
  saying what they mean, so a permissions failure never reads as a lock state.
- `verify` checks a report against the pair: it reads the report from
  `--upper` (provisional unless `--report` names one), fetches every listed
  item from both tenants and compares content, ignoring `_rev` and audit
  stamps. Each item is `same`, `DIFFERS` (with the first differing JSON
  paths), `ONLY-LOWER`, `ONLY-UPPER`, `absent-both` (only for an item
  reported `deleted`) or `unchecked` with the reason — an item it cannot
  read, including the global scripts it has no endpoint for, is never counted
  as matching. Exits non-zero if anything differs or is unchecked. Refuses two
  context names for one tenant.
- `status` treats `lock/state` `locked` beside the promotion status `READY` as
  agreement: that is how a held lock reads (measured 2026-10-27).

### The write cycle

`lock` → `dry-run` and/or `promote` → `unlock`, each sent to the **upper**
environment; a tenant that does not report both sides of a pair is refused.
Each verb starts only from a measured state, with every source agreeing —
proxies included — and `blockingError` reported `false`:

| Verb      | Starts from                                          | Settles at                                | Measured   |
| --------- | ---------------------------------------------------- | ----------------------------------------- | ---------- |
| `lock`    | unlocked everywhere                                  | locked both sides, status `READY`         | ~3 minutes |
| `dry-run` | locked, `READY`                                      | `READY` again, after `RUNNING` was seen   | ~8½ minutes |
| `promote` | locked, `READY`                                      | as `dry-run`                              | measured twice, not timed |
| `unlock`  | locked, `READY`, with a `promotionId` on the status  | unlocked everywhere                       | ~3 minutes |

Any other reading — a transition, one side ahead of the other, a running
promotion, a blocking error — is refused with what each source said. `lock`
refuses a lock already held (a second one is unmeasured, and the held one may
be someone else's); `unlock` releases the id the tenant reports, never one you
type, refuses while a promotion runs, and on a pair already unlocked says so
and exits zero.

`unlock --recover <promotion-id>` is the escape for a lock a failed promotion
left: it is accepted only while the status is `READY` with a blocking error,
over a lock every source agrees is held, and only when the tenant's
`promotionId` is exactly the one given. No failed promotion has been measured,
and a blocking error does not say work has stopped, so any other status — a
transition, `RUNNING`, a name never seen — is refused and left to the console.

**The state is read twice.** Once before the prompt, and again after consent,
the production gate and the report-list read; the write goes out only if the
second read still allows it under the **same** `promotionId`. A promotion that
started, or a lock replaced by someone else, while the prompt was open is
refused with nothing sent. This narrows the race; it cannot close it, because
no conditional write has been verified on this service. A request that fails
in flight is reported as possibly delivered — check `status` before retrying.

While waiting, every reading about a lock is judged under the operation's
`promotionId` — the approved one, or for `lock` the id its acknowledgement
names. A reading under a different id fails the wait: another operation has
taken the pair. One naming no id is not counted either way. A `lock` whose
acknowledgement names no id exits non-zero rather than wait, because nothing
could tie the result to the request — except under `--no-wait`, which reports
acceptance, not completion, and exits zero with a warning. A promotion counts as finished only on
`READY` **after** it was seen `RUNNING` under its id; a run too quick to be
seen is reported unconfirmed when the timeout expires.

Each asks for confirmation at a terminal; `--force` proceeds without one, and
without either the verb refuses naming what it would do. `--yes` is the
production gate, checked after the state check and consent. By default the
command waits for the tenant to settle, printing each new status, and gives up
after `--timeout` minutes (15 for `lock`/`unlock`, 45 for `dry-run`/`promote`)
— the operation carries on there. `--no-wait` returns once the tenant accepts.

`dry-run` and `promote` finish by naming the report they produced, found by
difference from the report list read before the start (one lock is one
`promotionId`, so a dry run and a promotion inside it share it; the report's
`dryRun` flag tells them apart). The report list is read once the run has
finished, and is never what decides that it has. This is best-effort
identification: **no** new report exits non-zero, because what the run did is
then unconfirmed, and more than one is listed as candidates, since another run
shared the lock. Even one is a candidate, not proof.
Neither unlocks. A report can list items the promotion did not change — on a
pair with nothing to promote, a dry run's was byte-identical to the previous
real promotion's — so check one live with `verify --report <id>`.

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
`--dry-run` prints the rule-level change summary without writing, confirming, or
creating a backup. Writes prompt after showing the summary unless bare `--force`
is supplied; global `--no-prompt` therefore requires `--force` for a real write
(and production additionally requires `--yes`). See
[Flag migration](#flag-migration) for the removed `--no-backup` spelling and for
unattended commands that used `--yes` to accept the summary.

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
aic oauth pull <id> [--realm alpha] [--force] [--force=backup] # protected workspace install
aic oauth push <id> [--realm alpha] [--force] [--yes]   # push; production requires --yes
aic oauth diff <id> [--local-vs-snapshot | --snapshot-vs-remote]  # compare two versions
aic oauth delete <id> --force [--realm alpha] [--yes]   # delete; production requires --yes
```

`pull` protects local client edits with the same content-based rule as `push`,
with recursive `_rev` fields ignored. A missing export installs normally; an
export equal to the tenant or its valid snapshot is safe. Otherwise, including
malformed local JSON or a missing/malformed snapshot, replacement requires bare
`--force` or an affirmative default-no terminal confirmation. Every differing
existing export is backed up under `.aic-sync/backups/` before replacement. Bare
`--force` still takes the backup; only `--force=backup` skips it, and that named
scope does not authorize overwriting local edits. Backup names include
oauth/realm/client plus timestamp and UUID, and files are mode 0600 on Unix.
Snapshot writes happen after the export install.

`provider get` prints a compact realm-wide configuration summary, resolving
script ids to names the same way `get` does. It always shows a row for both the
provider `grantTypes` and `tokenExchangeClasses` — `<absent>` when the tenant
does not set one — because configured exchangers do not themselves enable the
token-exchange grant. It also derives a direct `token-exchange granted` yes/no
row, from `grantTypes` alone. Arrays inside the known configuration groups
expand to one value per row and the `[Empty]` sentinel prints as `<not set>`; a
group this command does not know about, and an object-valued plugin setting,
still print as a single JSON cell. Token-exchange class mappings drop their
repeated token-type URN and Java-package prefixes. That rewrite is purely
structural, so a mapping that does not parse is printed in full — but one naming
an unfamiliar token type or exchanger class is shortened like any other.
`--json` prints the same document the API returned, re-serialised rather than
passed through byte-for-byte.

`list` prints one row per client: id, client name, type, status and grant types.
A field the tenant does not set shows as `-` — some clients really do have
`status: null`. `--filter TEXT` keeps the clients whose id **or** client name
contains TEXT, case-insensitively; both, because the two disagree in practice
and either can be the one you remember.

`--no-dynamic` hides clients whose id is a bare UUID, which is what AM mints for
a client registered through dynamic registration — on a tenant with a thousand
DCR clients that is the flag that makes the list readable. It tests the **id**,
not the provenance: AM records nothing saying a client came from DCR, so a
hand-made client given a UUID id is hidden too, and a dynamic registration that
supplied its own `client_id` is not. That is why a filtered listing always
reports how many rows were hidden and by which flag. Verified 2026-09-10 against
a real dynamic registration: the client AM created carried a UUID `client_id`,
and `--no-dynamic` hid exactly that row.

`--json` still prints ids alone, not the new columns, so anything piping it into
another command keeps working.

`exchange list` answers the question RFC 8693 configuration is spread too thin
to answer by reading: **who may act for whom.** The realm decides whether the
grant exists at all, which token types have an exchanger, and which script
stamps `may_act` by default; each client decides whether it holds the grant
(making it an **actor**) and whether it stamps `may_act` on the tokens it issues
(making it a usable **subject**). One client can be both. By default a client is
listed when it says something the realm header does not: it holds the grant (or
that could not be determined), it configures a may-act script, it has a **live**
override block while the realm sets a may-act script — an exception to the
realm-wide one — it sets its own audience values or a non-zero auth level, or
something in its configuration could not be read. The tally says how many were
hidden; `--all` lists every client.

The subject half is the **effective** answer, not the configured one, and the
cases differ:

- a live client override with a script — shown as `subject`;
- **no live override** while the realm sets `accessTokenMayActScript` — shown as
  `subject (realm)`. A realm script makes _every_ such client a subject, so the
  header says it once rather than the table saying it a thousand times;
- a `(dormant)` override — configured under a `providerOverridesEnabled` that is
  not `true`, so it stamps nothing and the realm's script applies instead.
  Without a realm script, that client's tokens cannot be exchanged at all;
- a **live** override that sets no script — `providerOverridesEnabled: true`
  with the field on `[Empty]`. This is not silence: enabling the block stops
  inheritance for every field in it at once, so the realm's script does _not_
  apply and nothing stamps the claim.

Because the default view lists clients with exchange configuration **of their
own**, a client that is a subject only by inheritance does not appear without
`--all`. The header line is what tells you it exists.

`MAY-ACT SCRIPT` names the script per field — `access` and `oidc` stamp the
claim on different token types, and they collapse to `access+oidc` when they
name the same script. It prints a resolved script's **name** alone to keep the
row inside a terminal; an _unresolved_ id keeps its full UUID, because that case
is a finding, and `--json` carries every id regardless. `AUTH_LEVEL` is
`tokenExchangeAuthLevel` verbatim. `0` is AM's default; what a non-zero value
does is **not verified here** — the name and AM's use of "auth level" elsewhere
suggest a floor on the subject token, and `docs/api/22-token-exchange.md`
records it as untested. The column reports the number, not an interpretation of
it. `AUDIENCE` is `allowedResourceServerAudienceValues` — empty means the client
cannot be asked for an `audience` at all. `ACCEPT_AUD` is the client's override
of `acceptAudienceParametersInTokenExchangeRequests`, which lives in the
**override** block and so is governed by the master switch (the other two are
not). It has four values, because three of them are not the same answer:
`yes`/`no` when the live block decides it; `realm` when the block is dormant, in
which case the header's `realm acceptAudienceParameters` line is what applies;
`block default` when the block is live but the field absent, where the override
block's own default governs and this listing never sees it; and `?` when the
master switch or the field itself could not be read. Worth reading with
`AUDIENCE`, because a client that accepts no audience parameters silently
**ignores** an `audience=` it is sent rather than rejecting it.

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
token-type _pair_, and a forged subject token are all invisible here and all
produce the same message. Clearing these warnings narrows the search; it does
not end it. In particular there is no field naming _which_ actor a subject
permits — the may-act script decides that at mint time — so the relationship is
only fully readable by reading the script this command names.

A projected field that is **present with an unusable type** is reported, and the
answer it feeds is withheld rather than defaulted — `?` in a cell, `null` in
JSON beside a `…Readable: false`, never the `-` that means "not set". It matters
here more than elsewhere: a `grantTypes` that stopped being an array would
quietly turn an actor into a bystander in the one view whose job is to say who
can act, and a warning printed under "no client holds the grant" does not make
that sentence true. So an unreadable grant list gives `actor?` and suppresses
the no-actor finding; an unreadable `providerOverridesEnabled` gives `subject?`
and suppresses the deny-by-default finding; the realm document gets the same
treatment, where it matters more rather than less. The same applies to a listing
page that comes back with an unusable `_id` or `pagedResultsCookie`: both are
refused rather than skipped, because a listing that quietly loses rows or stops
early is how an incomplete population becomes a confident statement about every
client. An _absent_ field stays silent, because absent really does mean not set.

`--json` carries the realm's half, the projected rows and the findings. The
warnings also go to stderr, so a piped `--json` loses nothing.

Two JSON fields need care. `acceptAudienceParametersOverride` is the **client's
override**, never the runtime value — with the block dormant the realm's
`realmAcceptAudienceParameters` governs, and with the block live but the field
absent the block's own default does, which this projection never sees. And
`tokenExchangeGranted`, `actor` and `overridesLive` are all nullable: `null`
means the document did not say, which is not `false`. The table spells that
`actor?` / `subject?`.

`get` prints one client as a compact table and **writes nothing** — reading a
client used to mean `pull`, which drops a JSON file in the workspace and
overwrites the snapshot as a side effect. It shows what the client is (id,
status, name, type), how it authenticates (`tokenEndpointAuthMethod`,
`subjectType`), and what it may ask for (grant types, response types, scopes,
default scopes, redirect URIs, implied consent, the three lifetimes). Array
fields print one value per row. `--json` prints the document the API returned.

The last section is `overrideOAuth2ClientConfig`, whose 29 keys are only
meaningful alongside one of them: `providerOverridesEnabled` is a master switch,
so the section header says whether the block runs at all rather than echoing a
boolean — `in effect` when the switch is true, `dormant` when it is false **or
absent** (the switch has to be `true`; a missing one leaves the realm in
charge).

The rows are the same either way, deliberately. A dormant
`statelessTokensEnabled: true` is exactly what you need to see before enabling
the block to attach one script, because flipping the switch makes every value in
it live at once — including ones you never set. Entries that say "inherit" (the
`[Empty]` sentinel, `null`, an empty array, a `…PluginType` of `PROVIDER`, a
`…Class` still on AM's `Default…` implementation) are suppressed and **counted**
on a final row, so nothing is dropped without saying so. A script id prints as
`name (uuid)` where the realm has a script with that id, and as the bare UUID
where it does not. Only fields that hold a script id are rewritten — a scope or
a client id that happened to be UUID-shaped keeps its value. The lookup covers
**every** script in the realm, including the Groovy and product-internal ones
`aic script list` hides, because an override can legitimately point at one and
resolving it to nothing would report an existing script as missing — an id
naming a script that is not there is a finding, and replacing it with a
placeholder would remove the value you need to chase it. Resolution needs a
second request; if that fails you get the UUIDs and a warning saying why.

After the table, `get` warns on stderr about each `…Script` that cannot run: one
set while its `…PluginType` is still `PROVIDER` (or `JAVA`) is ignored by AM,
and a `…PluginType` of `SCRIPTED` with no script runs nothing. Both are accepted
silently by the tenant, which is what makes them expensive. The may-act fields
have no `…PluginType` companion — setting the id is enough — and are correctly
never reported. A warning from a dormant block is prefixed `(dormant)`: it is
not biting yet, and enabling the block is what would make it bite.

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
disagree about whether a client changed. Secret values are replaced by their
full SHA-256 — the `*-encrypted` blobs AM returns, and a plaintext
`userpassword`, which only ever appears on the local side because AM reads that
field back as `null`. `pull` already writes both to the workspace, but a
rendered diff also reaches your scrollback, your pager's history and any CI log,
and a digest still changes when the secret is rotated. The OAuth tab masks the
same fields in its detail pane, everywhere in the document.

A side that does not exist is reported on stderr and rendered as empty, and a
comparison where **neither** side exists (a mistyped client id) fails rather
than reporting the two absences as identical. `pull` is suggested as the remedy
only when no **local** side would be lost by running it — it rewrites both the
local file and the snapshot, and never the tenant, so it is the right advice
when the client is simply not pulled yet and the wrong advice when your edits or
your snapshot are the surviving side. In that case the note says what `pull`
would cost instead of naming it.

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

## `aic saml` — SAML 2.0 entity providers and metadata

Realm-scoped, CLI-only (no TUI tab). Eight verbs write: `create-hosted`,
`import`, `delete`, `cert add` and `cert remove` (a remote entity's signing
certificates), and the three halves of a hosted certificate rollover —
`rotate init`, `rotate stage` and `rotate complete`. There is no
circle-of-trust write verb — a later slice.

```bash
aic saml list [--location hosted|remote] [--role idp|sp] [--realm alpha] [--json]
aic saml show <ENTITY-ID> [--location hosted|remote] [--realm alpha] [--json]
aic saml cot list [--realm alpha] [--json]
aic saml cot show <NAME> [--realm alpha] [--json]
aic saml create-hosted <ENTITY-ID> --role idp|sp \
  --meta-alias /<realm>/<name> [--realm alpha] [--yes]
aic saml import <FILE> [--realm alpha] [--dry-run] [--no-sanitise] \
  [--certs add|replace] [--force] [--yes]
aic saml delete <ENTITY-ID> [--location hosted|remote] \
  [--realm alpha] --force [--yes]
aic saml metadata export <ENTITY-ID> [--realm alpha] [--out PATH]
aic saml metadata inspect <FILE>
aic saml metadata sanitise <FILE> [--out PATH] [--keep-signature]
aic saml cert list <ENTITY-ID> [--role idp|sp] [--realm alpha] [--json]
aic saml cert add <ENTITY-ID> --cert-file PATH [--role idp|sp] \
  [--realm alpha] [--force] [--dry-run] [--yes]
aic saml cert remove <ENTITY-ID> <SHA256-OR-PREFIX> [--role idp|sp] \
  [--realm alpha] [--force] [--dry-run] [--yes]
aic saml rotate status <ENTITY-ID> [--role idp|sp] [--realm alpha] [--json]
aic saml rotate init <ENTITY-ID> --identifier <NAME> --secret-id esv-<NAME> \
  (--key-file PATH | --key-stdin) [--description TEXT] \
  [--role idp|sp] [--realm alpha] [--force] [--dry-run] [--yes]
aic saml rotate stage <ENTITY-ID> (--key-file PATH | --key-stdin) \
  [--role idp|sp] [--realm alpha] [--force] [--dry-run] [--yes]
aic saml rotate complete <ENTITY-ID> [--retain <SHA256>] \
  [--disable-version <N>] \
  [--role idp|sp] [--realm alpha] --force [--dry-run] [--yes]
```

### Which verbs need an unlocked agent

Everything but `metadata` and `cert list`, `rotate` included — even
`rotate status`, which writes nothing: it correlates the entity, the secret
mapping and the ESV secret, and only the metadata export inside it is
unauthenticated. `cert list` reads nothing but that export, so it works locked;
`cert add` and `cert remove` list the realm over the bearer before they write.

`metadata inspect` and `metadata sanitise` are local file rewrites, and
**`metadata export` reaches the tenant over an endpoint that takes no
authentication at all** — verified by fetching it with and without a bearer and
comparing the bodies byte for byte (`docs/api/06-saml.md`). It therefore sends
no credential, never starts or unlocks the agent, and works against a locked
daemon.

### `list`

One `GET` of the realm's whole entity collection; `--location` and `--role`
filter the result client-side, because `?_queryFilter=true` is a **400** on the
`/hosted` and `/remote` sub-collections and works only on their parent. An
entity may hold **both** roles, and `--role` tests membership, so a dual-role
entity appears under `--role idp` and `--role sp` alike. An entity with no role
blocks is legal and lists with `-` for its roles.

The empty-list line names the realm it searched. The project default is `alpha`,
and a tenant's SAML entities commonly live in `bravo`, so "nothing here" must
not read as "this tenant has no SAML".

### `show`

`--location` is **inferred** when omitted, from one list call. The full read is
`…/saml2/{location}/{entityId64}`, and the _right_ id in the _wrong_ collection
answers 404 — which reads as "that entity does not exist". If the id is not in
the realm at all, the error says so and points at `list`.

Default output is a summary: entity id, location, and per role the `metaAlias`
and the signing secret identifier. `--json` prints the raw document. The summary
reads leaves, not keys: a full entity carries every group key with `{}` inside
when nothing in it is set, so key presence says nothing.

### `cot list` / `cot show`

Circle-of-trust ids are **plain names**, not base64url — only entity providers
are encoded. The list endpoint returns full documents, so `cot list --json`
prints what the tenant said; the table shows name, status, a _count_ of
`trustedProviders`, and the description (`-` when absent, which is its real
state — AM cannot store an empty one).

**Neither verb shows the membership that governs authentication, and both say
so.** AM records membership in two places: the CoT document's
`trustedProviders`, which REST returns, and each entity's `cotlist` extended
metadata, which REST never exposes — and the runtime trust check reads
`cotlist`. A provider listed here can still have its assertions rejected, and
one missing here can still authenticate (`docs/api/06-saml.md`). The caveat is
part of the human rendering and goes to stdout with it — including when the
realm has no circles of trust at all, where "none" is the reading most likely to
be taken as proof that nothing trusts anything. `--json` puts it on stderr
instead, so the JSON stream stays clean.

An entry with no `|protocol` suffix is shown and flagged rather than hidden: AM
stores and removes those with a 200 precisely because nothing can be resolved
from them, so they are inert, not invalid.

### `create-hosted`

`POST …/realm-config/saml2/hosted/?_action=create`, and every check it makes is
one AM does not:

- **The entity ID is required.** `_action=create` with `{}` answers **201** and
  AM mints a UUID-named entity that nothing will ever reference.
- **`--meta-alias` is required and must be `/<realm>/<name>`.** A role block
  without `services.metaAlias` fails with
  `500 Exception from invocation expected to be handled by promise`, which names
  no field, so there is nothing in the response to translate. Every endpoint AM
  publishes for the entity embeds the realm
  (`/am/AuthConsumer/metaAlias/<realm>/<name>`), so an alias in another realm's
  namespace is wrong in a way nothing reports.
- **An entity ID already in the realm is refused.** What AM does with a create
  against an existing id was never measured, and both outcomes it could have —
  an opaque 500, or replacing a configured entity — are worse than a named
  refusal.

One role per invocation. `roles` is derived from which role blocks are present,
so a dual-role entity is built by adding the second block later, not here.

The 201 body is a stub (`_id`, `_rev`, `entityId`), not the created document, so
the report says which half of it AM confirmed: the id is AM's, and the role and
the alias are what was sent and were **not** read back. An id that comes back
different from the one requested — the UUID-minting behaviour this command
exists to prevent — is a warning, not a silent substitution, and so is a 201
that carries no `entityId` at all.

### `import`

`POST …/realm-config/saml2/remote/?_action=importEntity` with the file
base64url-encoded into `standardMetadata`. Remote entities only: the same action
on `/hosted` is a **501**, and `?_action=create` on `/remote` is a 400 — import
is the only way a remote entity arrives (`docs/api/06-saml.md`).

**The URL-safe alphabet is mandatory.** The identical bytes in standard base64
answer `400 Invalid standard metadata value in request` — the same message as
sending `{}` or raw XML — so nothing in the response distinguishes a wrong
alphabet from an empty body from unparseable metadata. Padding is irrelevant.
The encoding therefore happens locally and is asserted locally.

**One file is not one entity.** An `EntitiesDescriptor` aggregate imports every
entity it contains in this one call and returns all their ids, so the command
parses the file into a bundle of _n_ entities and preflights, sends and reports
on all of them. That is also the one place the offline metadata tools are
narrower: `metadata inspect` and `metadata sanitise` take a single
`EntityDescriptor`, because a metadata **export** is never an aggregate and
widening the import path must not widen the classifier that decides whether an
HTTP-200 body is metadata at all. `import --dry-run` is how an aggregate gets
inspected.

**Any pre-existing entity id refuses the whole operation** — with the one
exception below — including the
entities that would have been created — an aggregate is one call, so there is no
partial send to offer. `?_action=importEntity` is **create-only**: a re-import
of an existing entity is a `500`, there is no upsert, and there is deliberately
**no `--force`** that deletes and re-imports. The delete would cascade through
every circle of trust that listed the entity, and the `cotlist` in extended
metadata — the half of CoT membership the runtime actually reads — is not
exposed by REST, so nothing could tell an operator what the "recovery"
destroyed. Remove an entity deliberately with `aic saml delete` if that is what
is meant.

**The one exception: a file whose single entity already exists as a _remote_
entity** becomes a certificate update — the same `importEntity` call with
`"updateType": "UPDATE_CERTIFICATES"`, which replaces that entity's certificates
in place and leaves its circle-of-trust membership alone (`docs/api/06-saml.md`).
It is `aic saml cert`'s machinery, so everything under [`cert`](#cert--a-remote-entitys-signing-certificates)
holds: the document sent is the entity's **own export** with signing
`KeyDescriptor`s spliced in or out, never the uploaded file, and a certificate
from the file reaches it as a key descriptor written fresh from its DER — after
the same X.509 check `cert add` gives a `--cert-file`, so a signing certificate
body that is not a certificate refuses the import, naming its line.

- The plan shows, per role, the signing certificates added, removed and kept
  (subject, SHA-256, `notAfter`), recomputed for the chosen mode before the
  final confirmation.
- `--certs add` merges the file's signing certificates in, deduplicated by
  fingerprint; `--certs replace` makes each role's signing certificates exactly
  the file's. Encryption and `use`-less key descriptors are kept either way.
  They are compared by role, `use`, `KeyName` and certificate SHA-256, in both
  directions and over every role either side declares: one only the file
  carries — including a certificate the file moved from signing to encryption,
  or one it only renamed — is listed as not applied, and one only the tenant
  carries, including in a role the file omits, is listed as kept. Nothing else
  inside a key descriptor is compared, and never its raw bytes: AM re-indents
  the whitespace inside `<KeyInfo>`.
- Every other difference is listed as **not applied — AIC ignores
  non-certificate changes on an update; changing them means delete and
  re-import, which drops the entity's circle-of-trust membership.** When
  nothing compared differs besides signing certificates, one line says so and
  names what was compared — the entity's and each role's attributes and every
  element nested in them (names, attributes and text, so a changed attribute deep
  inside an `<AttributeConsumingService>` counts), and the non-signing key
  descriptors as above. Whitespace is not compared, because AM re-indents what
  it stores; it is not a byte comparison.
- `--certs` chooses the mode and nothing else. Consent is separate, exactly as
  for `cert add`: a yes at a terminal, or `--force` without one. So at a
  terminal the flow is choose the mode (an add / replace / cancel menu when
  `--certs` is absent), read the plan, confirm; choosing cancel sends nothing
  and exits non-zero. Without a terminal (or with `--no-prompt`) it needs both
  `--certs` and `--force`, and refuses naming whichever is missing.
  `--dry-run` without `--certs` prints both modes' plans.
- A file that declares a role the entity lacks is refused: AM would add that
  role whole, endpoints and all.
- Everything else still refuses whole: an aggregate with any collision, a
  collision with a hosted entity, and a mix of new and existing entities.
  `--certs` or `--force` on a file that would not update a remote entity is
  refused rather than ignored. `--force` only ever confirms a certificate
  update: it never means delete and re-import, and there is no forced create.

There is **no `--cot`**. `{"standardMetadata": …, "cot": "<name>"}` returns 200
and leaves the named circle of trust unchanged, and a `cot` naming one that does
not exist is _also_ a 200 that creates nothing — unknown body fields are
discarded. A flag here would report a membership change that never happened.

The file is **sanitised by default**: SAML `RoleDescriptor`s whose `xsi:type`
resolves to a WS-Federation type are stripped, which is what Entra's
`federationmetadata.xml` needs before AM will take it. Removing a role changes
bytes the document's enveloped signature covers, so the signature goes with it —
the plan says that in as many words rather than dropping it quietly.
`--no-sanitise` sends the file verbatim.

`--dry-run` prints the plan — every parsed entity id with the roles it
publishes, the removal report, the preflight result, and the exact bytes that
would be sent by length and SHA-256 — and sends nothing. It stops by holding no
permission token rather than by returning in front of the write: the token is
minted only by the preflight, and the call requires one, so the preview path
cannot reach it.

Afterwards the command reports **only what AM said**. `importedEntities` is
compared as an exact **set** against the ids parsed from the file — a
matching count is not a match — and any id missing from one side or the
other, or named twice by AM, is a warning naming that id. The comparison is
byte for byte on purpose: AM echoes an Entra id ending in `/` back unchanged
(`docs/api/06-saml.md`), so there is nothing for a normalisation to repair
and two entities for it to conflate.

**Three endings get the fresh list of the realm, not one.** A failed call, a
`200` whose body names no `importedEntities` array, and a `200` whose
`importedEntities` is not the set the file declares are the same situation
from the operator's side: AM may have created every entity in the file and
said so in a shape this command does not read, or named entities that were
never sent. All three are followed by a list of the realm reported per
declared id, because a failed aggregate import is not a rollback and AM says
nothing about how far it got. The heading says which of the three happened;
the inventory below it is the same every time.

The third is the one that looks least like a problem, because the response
parsed. Having just declared that body untrustworthy, the command must not
then leave its contents as the account of what landed — that is precisely
what the relist replaces. An `importedEntities` **member** that is not a
string is the second case rather than the third: a number or an object means
the body was not understood at all, and rendering it as JSON text would feed
the set comparison an id AM never wrote.

Every run ends on the same sentence: **circle-of-trust membership was not
verified, and could not have been.** `importEntity` rewrites extended metadata,
which is where `cotlist` lives, and REST exposes it neither before nor after. A
post-import tick here would be exactly the state `docs/api/06-saml.md` warns
about — a federation that no longer authenticates, with REST showing a perfectly
healthy configuration.

`--yes` is the separate production-tenant confirmation, and it is read after the
preflight so that `--dry-run` can preview a production tenant without it.

### `delete`

**An entity `DELETE` silently rewrites every circle of trust that listed the
entity** — a member CoT was observed going to `[]` with no CoT write in between
(`docs/api/06-saml.md`). The operator cannot see that coming, so `delete` reads
the CoT collection first and prints, by name, each circle and each entry that
will go. A count would not do: the operator has to recognise them.

Afterwards it reads that collection **again** — always, even when the first
read found no circle naming the entity — and reports the difference, so the
cascade it prints is something it observed rather than something it predicted.
AM performs the cascade, not `aic`, and the delete response says nothing about
it. The circles reported as still listing the entity come from that second
read, not from the first, so one the pre-delete read never saw is still found.

**The exit code is about the cascade, not the delete.** The entity is gone
either way; a circle of trust that still lists it, or a re-read that failed and
so could not tell, exits **non-zero** with the report printed. Automation
reading a zero here is reading "the federation is consistent again", and that
is the claim being protected. A non-zero exit is never a reason to re-run the
delete — the message says so.

`--force` is required. The refusal path *is* the preview — without `--force`
the command performs both reads, prints the cascade, and writes nothing — which
is why there is no `--dry-run`: the preview holds no permission token, so it
cannot reach the write.

`--location` is inferred when omitted, exactly as for `show`. `--yes` is the
separate production-tenant confirmation and does not authorize the delete; it is
asked for **after** the `--force` refusal, so previewing a delete on a
production-themed tenant never demands it.

### `metadata export`

`GET /am/saml2/jsp/exportmetadata.jsp?entityid=…&realm=/<realm>`. Writes the XML
to `--out`, or to stdout.

**Failures come back as HTTP 200**, with no `Content-Type` and a plain-text body
beginning `ERROR : `, so the status code is useless and the body is what is
classified. A response that is neither a SAML `<EntityDescriptor>` nor an
`ERROR :` message is refused too, and quoted back — writing a login page or a
proxy error to `entity.xml` is exactly what the 200 invites. Nothing is written
unless the body is metadata, and "is metadata" means the whole body parses as
one well-formed `EntityDescriptor` document: a correctly namespaced start tag
with the response truncated after it is a failure, not an export. The two things
a _successful_ export may still carry are a missing `entityID` (a roleless
entity really does export a bare descriptor) and a certificate body this tool
cannot decode.

The request follows **no redirects**. `--out` is written from the body that
comes back, so a 302 would be enough to save another host's descriptor under
this entity's name; a redirect is reported as the failure it is.

`--realm` is always sent: omitting it on the wire selects the **root** realm,
not the current one.

### `metadata inspect` / `metadata sanitise`

`inspect` prints JSON describing the document: entity id, SAML 2.0 roles,
whether it carries a signature over itself, endpoints, certificate fingerprints,
and what `sanitise` would remove at default options.

`sanitise` splices the original buffer rather than parsing and reserialising. It
writes the result to `--out`, or to stdout if that flag is omitted. The removal
report goes to stderr, one line per element. Default options strip SAML
`RoleDescriptor`s whose `xsi:type` resolves to a WS-Federation type — the shape
Entra emits — and then any XML signature that cut leaves unable to verify.

**What a signature covers is read from its `<ds:Reference URI>` values, not
from where it sits.** `URI=""` is the whole document and `URI="#id"` is the
element declaring that XML ID; a signature is kept only when every one of its
references lands on bytes this rewrite does not touch. So a signature over one
role the rewrite keeps survives a sibling being stripped, and so does the
signature on a document that needs no other stripping — while a signature
nested under a retained role that references the whole entity is removed,
because stripping a sibling really does break it. Placement says nothing about
either case.

Three details decide what "its references" and "that XML ID" mean, and each
one is the difference between reading what XMLDSig establishes and reading
more certainty than it gives:

- **Only the signature's own `<ds:SignedInfo>` is coverage.** Core validation
  is defined over those references; a `<ds:Reference>` in a `<ds:Manifest>`
  is checked by the application, or by nothing, and the signature validates
  either way. So a manifest reference is not counted — counting one would
  manufacture coverage rather than over-count it, and keep a signature over
  bytes that had moved.
- **A reference's transforms decide its bytes.** The digest is over what the
  `URI` dereferences to _and then_ what the transform chain makes of that.
  Canonicalisation and the enveloped-signature transform leave the answer
  alone and are read as exact; anything else — XPath, XSLT, base64, an
  algorithm nothing names — resolves to nothing.
- **An unqualified `ID` is an XML ID only where a schema declares it**, which
  here means SAML's own: the two container elements, every role element, and
  `<AffiliationDescriptor>`. An extension element spelling `ID` does not lend
  a reference a resolution, because this tool reads no schema that would say
  it should. `xml:id` is an ID everywhere by specification and always counts.

Anything that cannot be resolved to bytes of this document — an XPointer, a
detached reference, an id nothing declares or two elements claim, a transform
we cannot evaluate, a signature whose only reference is in a manifest, one
with no reference at all, or one nested inside another — is treated as
covering everything and is removed as soon as anything changes. Only verifying
the signature could settle it, and handing a peer a document whose signature
has silently stopped verifying is the failure this whole command exists to
avoid. Removing a stale signature is itself a change, so a signature covering
another one goes when that one does.

`--keep-signature` keeps every signature and, if a cut invalidated one, prints
a warning that it is now stale. It is not "leave the file alone": the
WS-Federation roles are still stripped, which is the whole reason the
signature has gone stale.

### What `metadata inspect` / `metadata sanitise` refuse

Both refuse a document they cannot fully read rather than passing it through —
the same boundary `metadata export` classifies against, so the two cannot
disagree. One root `EntityDescriptor` and no more (`import` is the one entry
point that also takes an `EntitiesDescriptor` aggregate); a namespace binding
for every prefix on every element _and attribute_; nothing but comments,
processing instructions and whitespace outside the root; no entity reference
whose replacement text we would have to guess; and the XML 1.0 productions a
tokeniser walks past — legal element and attribute names, an XML declaration
with a version, a legal character everywhere a character appears, no literal
`]]>`, no character reference to something Unicode has no character for, and
no two attributes resolving to one expanded name.

**A `DOCTYPE` is refused**, wherever it appears and whatever it declares —
external subset, internal subset, or nothing at all. These tools read no DTD,
so forwarding one means passing along a document whose expansion they cannot
describe to whatever parses it next; "we do not resolve it" is a promise about
this parser and not about AM's. SAML 2.0 metadata has no use for one, so the
refusal costs no real document anything.

**Every entity has to be nameable.** `inspect` and `sanitise` refuse an
`entityID` that is absent, empty or whitespace-only, and say which of the
three it is: the id is what `import` preflights against the realm and matches
in `importedEntities`, so an entity that cannot be named cannot be reported
on. `metadata export`'s classifier is blind to all three by design — it reads
the document's grammar and no attributes — because a roleless entity's legal
export really does carry no `entityID`.

`inspect` reports a certificate by the role that publishes it. A dual-role
entity can publish `use="signing"` from both its IdP and its SP role with no
`KeyName` on either, so `use` and fingerprint alone name two different keys
identically. A `<KeyDescriptor>` outside a role descriptor is refused rather
than reported with a role it does not have. Extension content is left alone:
only a _direct child_ of `EntityDescriptor` is a role this entity publishes, so
an `<Extensions>` container holding something shaped like a WS-Federation role
is neither stripped nor counted.

There is no circle-of-trust **write** verb: a CoT `PUT` drives the entity-side
membership too and can return 500 having already written the document, so the
command that does it has to re-read and re-check (`docs/api/06-saml.md`).

### `cert` — a remote entity's signing certificates

A **remote** entity's certificates are in its metadata, and AM replaces them
with `POST …/realm-config/saml2/remote/?_action=importEntity` and
`"updateType": "UPDATE_CERTIFICATES"` (measured 2026-09-28,
`docs/api/06-saml.md`). A **hosted** entity's are not — they come from the
secret store — which is what [`rotate`](#rotate--roll-the-certificate-a-role-signs-with)
is for, and `cert add` / `cert remove` refuse a hosted entity and say so.

`cert list` reads the unauthenticated metadata export and prints a block per
certificate: role, `use` (`signing`, `encryption`, or `unspecified` for a key
descriptor with no `use`), SHA-256, subject, issuer, `notBefore`, `notAfter`
with the days left, and the key algorithm and size. Expired and
expiring-within-30-days certificates are flagged. `--json` is an array of the
same fields plus `daysUntilExpiry` and `expiry` (`valid`, `expiring-soon`,
`expired`). It lists hosted entities too.

`cert add` takes one PEM `CERTIFICATE` block or a DER file and refuses a file
holding a private key. A certificate the role already publishes for signing is
a no-op that says so and exits 0. `cert remove` takes a full fingerprint or an
unambiguous prefix (colons allowed) and considers **signing** certificates
only. Both need `--role` when the choice is ambiguous: `add` on a dual-role
entity, `remove` of a certificate both roles sign with.

`UPDATE_CERTIFICATES` is careless in four ways, and each is a guard here:

- **It replaces every `KeyDescriptor` of each role the document names,
  encryption included.** So the document sent is the entity's current export
  with signing `KeyDescriptor`s spliced in or out — never re-serialised — and
  every other key descriptor goes back byte for byte.
- **The document's `entityID` is the target, and the remote collection writes
  to hosted entities too.** So the entity is confirmed `remote` from the realm's
  list, and the sent document's `entityID` must match it exactly.
- **A role the entity lacks is added whole.** So the sent document's roles must
  be a subset of the ones the realm list shows.
- **It accepts a role with no signing certificate.** So every role must keep at
  least one `use="signing"` certificate that reads as X.509 — a `use`-less one
  does not count, since what AM does with it for signing is unmeasured, and nor
  does one whose body is not a certificate — and removing the last one is
  refused with **no `--force`**. Add the replacement first, or end trust
  deliberately with `aic saml delete`.

The realm list and the export are re-read immediately before the write, and a
change to the entity's location or roles, or a single differing byte of the
export, refuses it ("run the command again"). Afterwards the export is read again —
certificates are not in the entity JSON, and its `_rev` does not move — and the
exact `(role, use, sha256)` **set** is compared with the plan, never a count. A
mismatch or a failed re-read exits non-zero, says whether the write was
refused, accepted but unverified, or unknown, and names
`aic saml cert list <ENTITY-ID> --realm <realm>` as the read to do before
retrying.

Confirmation follows `rotate stage`: the plan, then `--dry-run` stops holding no
permission token, then `--force` or a terminal prompt, then `--yes` for a
production-themed tenant. AM re-indents what it stores, so the export after an
add-then-remove round trip holds the same certificates but need not be
byte-identical to the one before it.

### `rotate` — roll the certificate a role signs with

**An AIC SAML signing certificate is not stored in the SAML entity.** The entity
holds a `secretIdIdentifier`, which is a label into AM's secret store
(`am.applications.federation.entity.providers.saml2.<identifier>.signing`), and
on AIC that label is backed by an **ESV secret**. So there is nothing to upload
and no metadata to push: a rollover adds an ESV secret **version**, and the verb
spans `src/saml/`, `src/esv/` and `src/secretmap/`.

Measured end to end: map a `pem` ESV secret onto the label and the export
carries that certificate; add a second version and the export publishes **two**
`<KeyDescriptor use="signing">`; disable the old version and it drops back to
one, in single-digit seconds. No restart — provided the secret was created with
placeholders **off**. And on the measured SP path AM **signed** with the newest
ENABLED version by the first observation, no later than 12 s after it was added,
which is what makes `stage` the moment everything hinges on.

| Verb       | What it does                                            | Reversible?          |
| ---------- | ------------------------------------------------------- | -------------------- |
| `status`   | correlates five documents; surveys who shares the key   | writes nothing       |
| `init`     | sets the identifier, creates the secret, maps the label — replaces the published certificate | one-time setup |
| `stage`    | adds a version — **treat as the signing cutover**; both published | emergency signer restoration, then two disables |
| `complete` | disables the **old** version — one certificate again    | `esv secret enable`  |

**Treat `stage` as the cutover, not the preparation for one.** On the SP
AuthnRequest-signing path, AM signed with the newest ENABLED version of the ESV
secret by the first observation — no later than 12 s after the version was
added — measured 2026-09-22 over five rounds, including a probe that re-added
the *older* certificate as the newest version and watched it take over again
(`docs/api/06-saml.md`). The newest ENABLED version was also always the first
listed in the export, so whether AM selects by recency or by that order was not
separated; the consequence is the same either way. **IdP assertion signing was
not measured**, and is assumed to behave the same — a safety assumption, not an
observation, and `stage`'s messages say which applies to the role in hand. So by
the time `stage` returns, treat this role as signing with the new certificate:
a peer that does not already trust it starts rejecting.

The safe order is therefore:

1. generate the new key pair locally — `stage --key-file` takes **your** key
   pair, which is what makes the rest of this possible;
2. give the new certificate to the peer and have them trust it, out of band;
3. `stage` — confirm at the prompt, which names the new certificate, or pass
   `--force`; treat signing as cutting over here;
4. `complete` — stops publishing the old certificate; on its usual path it is
   not expected to change what signs.

`stage` and `complete` are **separate operations, and that is not a style
choice**: `aic esv secret disable` cannot disable the latest version
(`400 Cannot disable latest secret version`), so completion disables the old
one. The interval between them is a peer that *refreshes* metadata catching up
from the two-certificate export — genuinely useful, and a human interval rather
than a timeout, but a catch-up rather than the safety step. The safety step is
number 2.

**`init`, `stage` and `complete` confirm the same way.** Each is a certificate
change — `init` because mapping the label *replaces* the published default,
with no two-certificate catch-up at all — so each prints its plan, then asks at
a terminal (the prompt names the certificate coming in, or for `complete` the
version going out), and refuses without one unless `--force` is given. The
production `--yes` gate is asked after that, and answers a different question.
`--dry-run` never reaches the prompt.

**There is no un-stage.** Disabling the version you just added is the 400 above.
What works is **emergency signer restoration**: add the old key pair again as a
**newer** version — `aic esv secret add-version <secret> --value-file
old-pair.pem` — which was measured at ~7 s and destroys nothing. It needs the
**old private key**, not only its certificate: secret values are write-only, so
a pair that was not kept cannot be read back. And it is half of the way back:
run straight after `stage` it leaves three ENABLED versions (the one that was in
service, the staged one, the restored copy), which `rotate` reports as
`inconsistent` and will not finish. Settle it by disabling the two superseded
versions — neither is the latest any more — with `aic esv secret disable
<secret> <staged>` and `aic esv secret disable <secret> <previous>`. `stage`
prints this path with the numbers filled in, before it asks and again after it
lands.

**Nothing here destroys anything.** `complete` disables, which
`aic esv secret enable` undoes. Destroying a version and deleting a mapping are
irreversible and stay explicit and elsewhere (`aic esv secret destroy`,
`aic secretmap remove`).

**A rollover preserves the identifier and its mapping.** Repointing the entity
at a new identifier would rotate nothing until a new secret was made, and would
leave the old label mapped with nothing naming it — a mapping outlives the label
that minted it, and the labels vanish from the schema enum the moment the entity
stops naming them. `rotate init` refuses an entity that already points somewhere
else for exactly that reason.

`--role` is **required on a dual-role entity**: each role has its own
identifier, its own labels and its own `KeyDescriptor`s, and both can publish
`use="signing"` with no `KeyName` — AM emits none — so `use` plus fingerprint
names two different keys identically.

`--key-file` (or `--key-stdin`) takes `cat key.pem cert.pem`, and it is checked
offline before anything is sent: exactly one private key PEM block followed by
one certificate PEM block, and the certificate's public modulus and exponent
must be the ones the key carries. The tenant takes a mismatched pair with a
**200** and the failure then surfaces on the peer as a rejected assertion, hours
later.

"Exactly one of each" is a claim about the armour, so it is worth being precise
about what passes:

- Armour that never closes, closes under a different label, or nests inside
  another block is **refused by name**, not skipped. That matters for a third
  document rather than a second: `cat key.pem half-written.pem cert.pem` leaves
  one key and one certificate once a broken block is dropped, so it would
  validate while missing the certificate you meant to include.
- Text **outside** the armour is ignored and kept. An `openssl x509 -text` dump
  or a dated comment above the key is fine — the value is stored verbatim, and
  AM parses the blocks out of it.
- Each block's DER must be one whole document. Trailing bytes after the
  certificate are refused, because the SHA-256 this command reports is a digest
  of exactly those bytes and has to be the same identity the metadata export
  publishes.

The private key has to be a whole key, not just the public half of one. An
`RSAPrivateKey` carrying only `version`, `modulus` and `publicExponent` matches
the certificate exactly as well as a real key does and cannot sign anything, so
all nine INTEGERs of RFC 8017 A.1.2 are required. That is a claim about
structure, and it is the last one this check makes.

What is **not** checked, because nothing offline could: no signature is
verified, no validity date is read, no chain is built and no key strength is
judged. Those are policy. The walk reads structure in order to prove the two
halves belong together; it is not certificate validation and does not stand in
for it. A non-RSA certificate is refused outright rather than passed unchecked,
for the same reason.

`--dry-run` prints the plan and sends nothing. It stops by holding no permission
token rather than by returning in front of the write: the preview arm of
`spec::authorize_*` carries no permit, every tenant write in `rotate::ops`
requires one, and a preview that fell through would not compile — the same shape
as `import`'s `ImportPermit` and `scripts::gate`'s `WritePermit`.

#### One secret and one role, or no rollover

**AIC permits a secret label to back several providers**, and a rollover is a
change to the ESV *secret*, not to an entity: `stage` adds a version and every
consumer starts publishing a second certificate; `complete` disables one and
every consumer stops publishing what that version held. Everything else here —
the phase, the export check, the settlement — reads only the entity named on
the command line. So a shared secret is how a completion that reports success
becomes an outage for the provider nobody looked at.

Every verb therefore surveys **every realm** — `alpha` and `bravo` — before it
plans, and refuses anything but "this role and nobody else". Two channels bring
a second consumer, and the survey covers both:

- a second entity — or a second **role** of the same entity — carrying the same
  `secretIdIdentifier`. AM enforces no uniqueness on that field, and the label
  is minted from it, so they resolve the same key.
- a second label mapped onto the same ESV secret. That includes the
  `encryption` and `mtls` labels off this very identifier, and a label with
  nothing to do with SAML at all.

**The refusal names them**, each with the label it resolves through: "this
secret is shared" is a dead end, and "`https://sp-b.example.com` resolves it
through `…spb.signing`" is a diagnosis. A label mapped onto the secret that no
entity provider in its realm names is listed too — it resolves nothing today,
and it is one entity delete away from being claimed again. Each is named with
its realm: the same entity ID in `alpha` and `bravo` is two consumers.

There is **no `--force`**. A shared rollover is not one operation with a risk
attached; it is several this command cannot sequence, because adding a version
is treated as cutting **every** consumer's signing over at the same instant — so every one of
their peers has to have been given the new certificate before it happens, on
schedules this command knows nothing about. Either give the role a label and an
ESV secret of its own, or roll the shared key deliberately: once **every** peer
above holds and trusts the new certificate, `aic esv secret add-version` (which
is where signing changes for all of them), then `aic esv secret disable`.

`rotate status` reports the same survey whether or not anything is shared,
because that is where every one of those refusals sends you. It costs, per
realm, the mapping table, the entity list and one read per entity provider —
`4 + N` calls for N providers across both realms: the entity list is stubs
only and carries no `secretIdIdentifier`, and there is no query filter for one,
so each provider has to be read to find out what it names.

**The write verbs survey twice.** Once to plan — so a shared secret is refused
before anything is printed as a plan — and again inside the pre-write recheck,
after the confirmation prompt and the production gate, because a consumer added
while you read the prompt would otherwise be cut over and never verified. The
write holds the proof minted by that second survey; the first is consumed by
authorising the plan. Every writer `init`, `stage` and `complete` reach takes
that proof as a required parameter, so a write path with the survey left out
does not compile — which guards against omitting the check, and does not make
the survey atomic or prove it is still current when the write lands. So `stage` and `complete` spend `2 × (4 + N)` calls on
the question. `init` can spend one survey more, `3 × (4 + N)`: it has up to
three writes, and the one that **activates** the chain — the last planned step,
from which the role resolves the secret; usually the mapping, or creating the
secret when a mapping already points at it — re-surveys immediately before it
is sent unless it is also the first step. Without that, a second entity taking
the identifier after step one would be cut over along with this one. This
narrows the race to one round trip; it cannot make it atomic, because no REST
read followed by a write can.

The survey is **tenant-wide over the realms this tool addresses**, and a survey
that skipped one is refused rather than trusted. What it still cannot see is
the **root** realm: AIC answers 403 for every root realm-config family measured
so far, but whether root can hold a SAML entity provider or a secret mapping
has not been measured, and every sharing report says so — every refusal, and
`status` in text and in `--json` alike, whether or not anything is shared.

**`rotate status --json` changed shape** when the survey became tenant-wide:
`sharing.unclaimedLabels` is now an array of `{realm, label}` objects (it was
an array of label strings), each `sharing.others[]` entry gained `realm`, and
`sharing` gained `realms` (what was surveyed) and `unsurveyedRealms`. A
consumer of the old shape has to be updated; nothing in this repository was
one.

#### What `status` cannot tell you

**Nothing readable says which published certificate came from which ESV secret
version.** Secret values are write-only and AM emits no `<ds:KeyName>`, so
during a two-certificate window the export is two anonymous fingerprints. The
metadata lists the newest ENABLED version first, but that is an ordering
convention, not an identity — and a `complete` that trusted it would be one
convention-change away from retiring the new certificate and keeping the old.

What *is* known, as a rule to act on, is which certificate to treat as **in
use**: the newest ENABLED version's — observed on the SP path, where it was also
always listed first, and assumed for the IdP. `status` prints that as a rule
rather than pointing at one of the fingerprints in front of it, because pointing
would mean reading the export's order — the inference everything else here
refuses. The two claims are different: which version holds a certificate is
unreadable; which certificate signs follows from a measured rule.

So `stage` records the pairing locally, in `.aic/saml-rotations.json`, and only
**after the tenant's own export confirms** the new certificate is published —
never from the bytes it sent. `status` attributes a certificate to a version
only from that record, never by elimination: knowing that A is version 2 does
not make B version 1, because a third ENABLED version or a certificate arriving
from a label mapped elsewhere produces the same picture.

The journal is per install and is not a transaction log — everything else a
rotation needs is read from the tenant, and nothing refuses to proceed because
it is missing. A rollover staged on another machine leaves no entry, and
`complete` then needs to be told both halves.

#### `--retain` names a certificate; `--disable-version` names a version

They are **not** two ways of saying the same thing, and treating them as one is
how a rollover retires the certificate it was told to keep.

`complete` disables an ESV secret version, which retires whichever certificate
that version holds. `--retain <sha256>` says which certificate must still be
published afterwards — it is checked against the export before the write and
verified against it after — but it cannot select a version, because nothing
readable pairs a fingerprint with a version number. So the version comes from
one of exactly two places:

- **this install's record of the stage**, which is the one thing that holds the
  pairing. It works in both directions: keep what was staged and the other
  ENABLED version is disabled; keep what was in service before and the staged
  version is disabled.
- **`--disable-version <n>`**, which is your claim rather than the command's.
  The plan says so in as many words before it asks you to confirm.

They are not alternatives you choose between. **When there is a usable record
it decides, and `--disable-version` may only agree with it.** A named version
that disagrees is refused, because with two ENABLED versions publishing two
certificates the one the record does not name is the one holding the
certificate `--retain` just asked to keep — so the flag would retire it, and
nothing before the write would have said so. If you believe the record is
wrong, move `.aic/saml-rotations.json` aside and name both halves, rather than
overruling half of it.

A record counts as usable only while it still describes the tenant in front of
it: the role must still point at the identifier that was staged, that
identifier's signing label must still resolve to the same ESV secret, that
secret must still hold the version the record names **as an ENABLED one**, and
the certificate it names must still be published. An entity deleted and
recreated under the same id, from the same key pair, keeps the fingerprint
matching while "version 2" comes to mean something the record has never seen —
so a fingerprint alone is not the test.

ENABLED rather than merely present, because a disabled version publishes
nothing and so ties no published certificate to anything. The case that
misleads is the same certificate material in two versions, one disabled and one
enabled: everything weaker passes, and the version named is the one publishing
none of it. `complete` was already safe — it derives only from ENABLED versions
— but `status` reads the same rule, and the number it prints is the number you
then type into `--disable-version`.

With no usable record, `complete` refuses until it has both. That includes the
case where picking by age would have been right — from inside the command that
case is indistinguishable from the one where it is wrong, and "it happened to
agree" is not knowledge. `aic saml rotate status` lists the versions and the
fingerprints side by side.

Keeping the older certificate usually means retiring the newer one, whose
version is the latest — and AIC will not disable the latest version
(`400 Cannot disable latest secret version`). There is no flag for that: stage
the key pair you want to keep as a new version and complete that rollover
instead. Where it *is* reachable — a DISABLED spare version above the staged
one — that `complete` disables the version to treat as signing, so **treat it
as a signer cutover** back to the retained certificate, with `stage`'s peer
precondition. That is a prediction from the rule, never measured on either
role, and the plan and the prompt say so in that form.

What the export cannot do is **attribute**. It says what is published; which of
two published keys the runtime picks follows from the rule above — the newest
ENABLED version, observed on the SP path — and not from anything in the
document.

#### Resuming an interrupted run

By re-running. `init`'s three steps are skipped individually when the tenant
already shows them done, so a run cut off between the entity `PUT` and the
mapping finishes the mapping and nothing else. `stage` and `complete` read the
phase first and refuse from a state they cannot act on, naming it. Nothing
performs compensating destructive cleanup on its own.

The one thing that cannot be re-derived is the version/certificate pairing, and
that is what the journal holds. Only `stage` writes to it: `init` opens no
two-certificate window, so it has no pairing to record, and an entry claiming
otherwise would be read by a later `complete` as the rollover this install
staged.

**Every write re-reads what it was decided from, immediately before sending.**
A plan can be minutes old by the time it is acted on — `complete`'s waits
through a confirmation prompt, which is a human interval by design — and a
version number outlives a change to the versions while the certificate it holds
does not. So `stage` and `complete` re-read four things: the role's
`secretIdIdentifier` and that identifier's mapping, which together must still
resolve to the ESV secret the plan named; and then the secret's ENABLED
versions and this role's published certificates, which must still be the ones
the plan was decided from — and between the two, they **re-survey every realm**
for other consumers of the secret (above). `init` does the same in the same
order before the first of its three steps — the `secretIdIdentifier` and the
label's mapping must still be what the plan saw, then this role's published
certificates must still be the ones the confirmation named as being replaced,
then the re-survey — surveys again before its activating step (above; not the
published certificates, which `init`'s own earlier steps can legitimately
change), and each step re-reads its own
document before overwriting it. A refusal before the first step sends nothing.
A stop **after** a step has completed is reported the way `.ai/core.md` §5 says
a batch that stops partway must be: it names the steps that completed
(`2 of 3 init steps completed — …`), says they were sent and not undone, and
does not claim that nothing was sent.

The step that stopped the run is reported separately, in one of three states,
because "it failed" is two different claims:

- **not applied** — refused before it was sent (a recheck, the production
  gate, a locked agent), or answered with a refusal **measured** for that
  endpoint: `400 "Failed to create secret, the secret already exists"` for the
  secret create, `400 "Cannot disable latest secret version"` for the disable
  (`docs/api/03-esvs.md`). No status class earns this on its own — a `408` or a
  `409` is **unknown**, and the client follows redirects, so the status seen
  need not answer the write that was sent. `init` treats the role as untouched
  only when **no step completed at all**. After any completed step — even
  creating a secret nothing mapped to yet, since another writer can map the
  label in the gap and no survey can prove it did not — it names what
  completed, says signing is unmeasured, and says to read the tenant.
- **accepted but not verified** — the tenant answered success and proving what
  it left then failed: the entity read-back, the export, the response's version
  number. The write landed.
- **outcome unknown** — attempted and possibly sent (the bearer is minted
  before the request, and a minting failure looks the same), and what came
  back does not settle it: any error
  status that is not a measured refusal, a transport failure, a lost agent
  connection, or a success whose body could not be decoded (the agent reports
  that one without its status).

For the last two the message says to read the tenant before retrying, makes no
claim about the chain, and — when the step was the activating one — says to
treat signing as possibly cut over. `stage` and `complete` report their write,
and every post-write check after it, in the same three states and the same
words; a re-sent version or disable that had already landed is a second write,
not a retry. In every case the remedy is `aic saml rotate status` followed by a
re-run, which plans from what the tenant shows.

The first two are not a formality, and leaving them out was a real gap.
Published metadata carries no secret and no version attribution at all, so
certificate equality cannot establish *which* secret is publishing a
certificate: a role repointed in the gap publishes exactly the set the plan
expected, while the secret about to be written is somebody else's. What is
required is that the fresh identifier and the fresh mapping still **resolve to
the planned secret** — not that they are unchanged. An identifier moved to a
label backed by the same secret rotates the same key, and refusing that would
leave a rollover stuck with two certificates published.

#### When `init` adopts an ESV secret instead of creating one

If the secret named by `--secret-id` already exists, `init` adopts it: it does
not write a key pair into it, and `--key-file` is then not used at all. That is
what makes an interrupted run resumable, and it is also the step whose success
`init` can say the least about — secret values are write-only, so nothing here
has seen the certificate behind the label.

So the claim is narrowed to what a read supports. After adopting, `init`
re-exports the metadata and requires the role to publish **at least one**
signing certificate; a label backed by a secret with no ENABLED version
publishes nothing, and that used to be reported as done. What it prints is the
fingerprint the tenant published, followed by a plain statement that nothing
here can confirm it is the certificate you meant. If a `--key-file` was
supplied, it also says whether that certificate is among the published ones —
because it was not written, and an operator who assumes otherwise finds out from
a peer.

An `init` that finds **all three** steps already done goes through the same
check rather than around it. "Already set up" is a statement about three
documents, and a label backed by a secret with no ENABLED version satisfies all
three while publishing nothing — which used to print "already set up" and exit
zero. The export is read first and the claim made afterwards.

Adding a key pair to a secret that already exists is `rotate stage`, not
`rotate init`.

#### `rotate init` and production

`init` maps a secret label, which `aic secretmap` restricts to
sandbox/development tenants — mappings are static content promoted up from lower
environments. `stage` and `complete` touch no mapping and work anywhere: a
production signing key still has to be rotatable.

#### A worked rollover

```bash
openssl req -x509 -newkey rsa:2048 -nodes -days 825 \
  -keyout new-key.pem -out new-cert.pem -subj "/CN=sp-a"
cat new-key.pem new-cert.pem > new-pair.pem

# Give new-cert.pem to the peer and have them trust it BEFORE staging:
# `stage` is the signing cutover, so a peer that does not hold it by then breaks.

aic saml rotate status https://sp-a.example.com --realm alpha
aic saml rotate stage  https://sp-a.example.com --realm alpha \
  --key-file new-pair.pem --dry-run
aic saml rotate stage  https://sp-a.example.com --realm alpha \
  --key-file new-pair.pem

# Signing has now moved to new-cert.pem. The old certificate stays published so
# a peer that refreshes metadata can catch up — a recovery, not a preparation.
aic saml metadata export https://sp-a.example.com --realm alpha --out sp-a.xml

aic saml rotate complete https://sp-a.example.com --realm alpha --force
```

---

## `aic secretmap` — AM secret-label → ESV-secret mappings

Realm-scoped. Re-point AM secret _labels_ (purposes) at existing ESV secrets.

```bash
aic secretmap list [--realm alpha] [--json]            # configured mappings
aic secretmap list-labels [--realm alpha] [--json]     # valid AM secret labels (alias: labels)
aic secretmap get <secret-label> [--realm alpha]       # one raw mapping
aic secretmap set <secret-label> <esv-secret-id> [--realm alpha] [--force]
aic secretmap remove <secret-label> [--realm alpha] [--force]  # alias: delete
```

`remove` deletes a mapping that exists on the tenant, even if its label is no
longer in `list-labels` (SAML entity identifier changes leave such orphans).
`set` still requires a label from `list-labels`.

---

## `aic workspace` — typed script workspace scaffold

Scaffold and refresh the local **typed workspace** at `./workspace/<tenant>/`
(one tree per tenant) with `.d.ts` definitions + ESLint/TypeScript config, so
your editor gets full IntelliSense on script bodies.

```bash
aic workspace init                            # scaffold the tenant tree (both realms + idm + typescript)
aic workspace update                          # refresh bundled types/config to the latest
aic workspace update --dry-run [--tenant T]     # preview versions and file changes; write nothing
```

`update --dry-run` lists files it would create, change, or remove, plus seed
drift and managed/sync-mapping type staleness. It writes nothing, including sync
state and generated types. Schema fetch failures only warn.

Without `--dry-run`, both commands also regenerate the tenant-derived types:
ambient `idm/types/managed/*.d.ts` for the `.cjs` scripts, and the module-form
`typescript/src/generated/managed.ts` for the TypeScript endpoint project.

Each names any **dangling reverse** it finds — a relationship whose
`reversePropertyName` points at a property the target object does not have. The
stock `alpha_application`/`bravo_application` objects ship with six. The reverse
side is missing from the runtime too, so the omission in the generated types is
correct; the warning exists so a member you expected and cannot find reads as a
tenant schema defect rather than a generation bug
(`docs/api/10-managed-objects.md`).

`update` refreshes every managed file and **adds the TypeScript project to a
workspace that predates it**, seeding its example endpoints. A seed you have not
edited is refreshed when the template moves; a seed you have edited, or one
whose origin cannot be verified (no recorded hash), is left alone and named in
the output. Deleted seeds stay deleted. `typescript/package.json` is merged
rather than replaced — the framework's toolchain entries are refreshed, any
dependency you added is kept.

`update` also removes **generated-only** script folders: a leaf `tsconfig.json`
(and, for AM libraries, the `export * from "./….cjs"` wrapper) left behind when
a script was deleted and recreated under a different context, or when the `.cjs`
was later removed by hand. A folder that still contains a script source, or any
file this tool did not generate, is left alone. Safe to re-run; a workspace with
no orphans is a no-op. `aic script delete` still keeps the local `.cjs` — that
is the user's source, not scaffolding.

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
`managed` (hook name is `<object>.<hook>`, e.g. `managed/alpha_user.onCreate`;
the workspace-path form `managed/alpha_user/onCreate` names the same hook).
So you never pass `--kind`/`--realm` to script commands. A bare `<name>`
resolves its namespace from your current directory. A bare namespace (`bravo`,
`endpoint`) means "all of it"; `all` means everything.

### Commands

```bash
aic script list [<ref>] [--context TEXT] [--default | --no-default] [--json]
aic script create <ref> --context <ctx> [--from FILE] [--language LANG] [--evaluator-version V] [--description TEXT] [--id UUID] [--tenant TENANT] [--yes] [--force=syntax-check]
aic script copy <src-ref> <dst-ref> [--tenant TENANT] [--yes] [--force=syntax-check]
aic script rename <realm>/<name> <new-name> [--tenant TENANT] [--yes] [--force=syntax-check] # AM only; keeps the id
aic script ids <DIR> [--realm REALM] [--tenant TENANT] [--json] # repo vs tenant script UUIDs, read-only
aic script delete <ref> --force [--tenant TENANT] [--yes]
aic script pull [<ref>] [--unmodified-only] [--force] [--force=backup] # protected pull; no ref → fuzzy picker
aic script push [<ref>] [--force] [--force=syntax-check] [--yes] # independent convergence/syntax permissions
aic script sync [<ref>] [--resolve local|remote --force] [--remove-local] [--force=syntax-check] [--tenant TENANT] [--yes] # prompts on remote deletion
aic script watch [--tenant TENANT] [--yes] [--force=syntax-check] # auto-push each .cjs you save (Ctrl-C to stop)
aic script status [<ref>] [--against FILE] [--check] [--json] # in sync / modified / remote / conflict / converged / deleted on remote
aic script diff [<ref>] [--local-vs-snapshot | --snapshot-vs-remote | --find-in-git REPO]
aic script who <ref> [--history] [--minutes N] [--json]   # who created/last modified it
```

When `sync` finds a synced script missing on the tenant, it prompts to skip,
re-create from the local source, or forget the sync record while keeping the
local file. `--resolve local` re-creates standalone resources with their saved
id; `--resolve remote` forgets the record for missing resources. Embedded
managed-hook and sync-mapping scripts cannot be re-created by this prompt; the
prompt names the owning config or missing local file when re-create is unavailable.
Skipped entries are listed and make the command exit non-zero.
`--remove-local` makes forgetting also remove the local file, but only one
that still holds the last-synced bytes (and a LIBRARY script's generated
wrapper with it, when that is a plain file holding exactly the generated
bytes); an edited file, a symlink, or a file under a symlinked folder is kept
and named. The record is
forgotten either way. It cannot be combined with `--resolve local`, which
never forgets.

- `status` with a full `<namespace>/<name>` selects exactly that script,
  case-sensitively, and prints the SHA-256 of the local file and of the
  tenant's copy (the digest `sha256sum` prints). Anything else — `am`/`idm`, a
  namespace such as `endpoint` or `endpoint/`, or a fragment without a
  namespace — is a case-insensitive filter, as before. **Behaviour change:**
  `endpoint/foo` used to match `endpoint/foo-bar` too; search for the fragment
  `foo` to get that.
  - An exact ref that nothing tracked matches is looked up on the tenant
    (read-only): `exists remotely; no tracked baseline`, with the remote digest
    and whether the canonical local file matches it, or `not on the tenant`.
    Nothing is recorded; a push still has to start from a pull.
  - `--against FILE` compares the tenant's copy of an exact ref with any file,
    such as a build artefact, by digest.
  - `--check` makes it a deployment gate: exit non-zero unless at least one
    script was selected and every one matches the tenant. "Matches" is
    content: `in sync`, `converged` (local and remote carry the same change;
    only the snapshot is stale — `sync` records it, and it used to be reported
    as a conflict), or an untracked script whose local file has the remote
    digest. With `--against`, the file alone decides: the workspace row is
    still printed but not judged, so a CI checkout that never pulled the
    workspace can gate on the artefact it built.
  - `--json` prints the same rows with `state`, `tracked`, both digests and
    `matches_tenant`, plus an overall `ok`.
  - A row whose tenant copy changed since the last sync — `modified on
    remote`, `conflict`, or `converged` — also names who last wrote it and
    when, the same `last modified` line and `modified` object `script who`
    prints (`last_modified` in `--json`). AM only: IDM config objects record no
    author, so IDM rows carry none; `script who` explains the logs route.
  - A tracked script whose snapshot file has gone is reported as `no snapshot`
    rather than silently skipped.
- `pull --unmodified-only` refreshes only tracked scripts whose local file is
  untouched since the last sync, or already equals the tenant's copy. It is
  the way to clear a screen of stale `modified on remote` rows so real drift
  stands out: `aic script pull endpoint --unmodified-only`. Local edits are
  skipped and named. Scripts with no synced baseline — untracked, missing
  locally, or with an unreadable snapshot — are skipped and counted, never
  adopted.
  Nothing it installs is protected, so it never asks for `--force`; backups
  follow plain `pull`'s rule. It writes only the workspace, never the tenant.
- `list` tags each row with its `ref` and narrows three ways. `--context TEXT`
  keeps AM scripts whose context **or** workspace folder slug contains `TEXT`,
  case-insensitively — so `--context OAUTH2_VALIDATE_SCOPE` returns both the
  legacy context and its `_NEXT_GEN` sibling, and `--context decision-node`
  returns both engine generations while `--context decision-node-legacy` returns
  only the 1.0 ones. It matches no IDM script, because only AM scripts have a
  context. When the filter keeps nothing, the contexts the tenant actually has
  are printed on stderr.
- `--default` / `--no-default` split the listing on the **DEFAULT** column,
  which means _shipped with the product_ (AM's `default: true`) — the scripts
  `script delete` refuses to remove. It does **not** mean "the script this realm
  is configured to run". That question is answered by `aic oauth provider get`
  (realm-wide, e.g. `validateScopeScript`) and by the client's own
  `overrideOAuth2ClientConfig` (per-client). Both report a script UUID; resolve
  it against the `ID` column of a listing.
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
  the refusal was previously unreachable for exactly the contexts it exists for,
  and you found out at runtime. Now `--context OAUTH2_VALIDATE_SCOPE` creates
  under `OAUTH2_VALIDATE_SCOPE_NEXT_GEN` and says so on stderr. Where a context
  has no next-gen form at all — `AUTHENTICATION_SERVER_SIDE`,
  `AUTHENTICATION_CLIENT_SIDE` — it refuses and names what the context does
  support. Do not infer the engine from the context's name or its language list.
  The global context list says `SAML2_SP_ADAPTER` is `JAVASCRIPT` only, while
  the realm's own `contexts/SAML2_SP_ADAPTER` reports
  `evaluatorVersions: {JAVASCRIPT: ["1.0"], GROOVY: ["1.0"]}` — the two AM
  endpoints disagree, and only the second one answers the question being asked
  (measured 2026-09-10). That is why this is a live call and not a table. The
  check is keyed on the language the create will send, so `--language GROOVY`
  refuses everywhere a next-gen engine is required — every next-gen context
  advertises JavaScript alone.
- **Keeping script ids in step with a repository.** A configuration
  repository that pushes AM scripts by id (fr-config-manager's
  `realms/<realm>/scripts/scripts-config/<uuid>.json`) breaks quietly when the
  tenant holds the same name under another id: no content comparison sees it.
  Three verbs cover it:
  - `create --id <uuid>` creates an AM script with that id instead of a fresh
    one. The id must be a lowercase hyphenated UUID, the form AM writes;
    anything else is refused rather than normalised. A create is a `PUT` to the
    id, so an id the realm already has is refused — that would replace a
    different script. IDM kinds refuse `--id`: their ids derive from the name.
  - `rename <realm>/<name> <new-name>` renames an AM script and keeps its id.
    A `PUT` of the script's own config with `name` changed is a rename: AM keeps
    the id and the old name stops resolving (measured 2026-10-31). The script
    must be tracked and in sync — local file, snapshot and tenant identical —
    and not a product default; the new name must be free, on the tenant and
    in the workspace — anything at the new file or snapshot path, a symlink
    included, refuses the rename. The write goes through the same syntax gate
    as a push, is confirmed by a re-read, and the new name is installed from
    that read before the old one is forgotten, so a failure in between leaves
    the old name tracked. Running the same `rename` again then finishes it:
    when the tenant holds the tracked id under the new name and nothing under
    the old, it moves the workspace without writing to the tenant. Wherever
    the earlier attempt stopped, the second run accepts its own work — the
    new file already holding the renamed bytes, the new name already tracked
    under the same id, the old snapshot or old entry already gone — and
    refuses only an edit: either file holding anything but the renamed bytes,
    or a symlink at either path — and a new path that is the same file as the
    old one (a hard link, or a case-only rename on a case-insensitive
    filesystem), since finishing the move deletes the old paths. Identity is
    checked again after the install, because two names that were both
    absent can become one file when it is created; an aliased old path is
    then left alone rather than deleted. The old file is removed only
    if it still holds the bytes that were renamed. Scripts that load a library **by
    name** (`require('Old')`) are not found or changed; a `LIBRARY` rename says
    so.
  - `ids <DIR>` walks every `*.json` under `DIR` for AM script exports —
    objects carrying `_id`, `name` and `context` — takes the realm from a
    `realms/<realm>/` path component (`--realm` otherwise, default `alpha`),
    and compares each with the tenant's listing by name: `match`, `tenant id
    <uuid>` (same name, other id — the case this exists for), `tenant name
    <name>` (same id, other name: a rename on one side), `absent` (a push
    creates it), or a repository id that is not a UUID. It reads the listing
    only, never a body, and exits non-zero when any script disagrees; `absent`
    is not a disagreement.
- `watch` normally pushes only **tracked** scripts, and silently skips an
  untracked file. The one exception is an endpoint the TypeScript project
  declares it owns in `typescript/.aic-ts-manifest.json`: that has no snapshot
  precisely because it has never existed remotely, so watch **creates** it on
  the tenant (honouring the same prod guard as a push) and every later save
  takes the ordinary tracked path. Hand-written `.cjs` files are unaffected. If
  the tenant already has that name — a bundle built in another checkout, or a
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
  match” and a failed fetch/decode reports “confirmation failed”; both leave the
  local source and snapshot unchanged, describe the tenant state as uncertain,
  and exit non-zero.
- **Explicit sync resolution applies to every selected entry.**
  `--resolve local --force` makes the tenant match each existing local source
  via the same forced, syntax-checked, confirmed push; a missing local file is
  an error and is not restored. `--resolve remote` makes every local source
  match the tenant, backing up differing existing source first. Without
  `--resolve`, sync keeps its three-way behavior: local-only changes push,
  remote-only changes pull, equal changes converge, and genuine conflicts prompt
  when a terminal is available. Resolution and bare force require each other:
  `--resolve local` without `--force`, and bare `sync --force` without
  `--resolve`, both fail. Ordinary reconcile can still use
  `--force=syntax-check` by itself.
- **Pull backups are based on the bytes being replaced, not the snapshot.** A
  normal `pull`, a reconcile pull, and `sync --resolve remote` write any
  differing existing source to `.aic-sync/backups/` before replacing it, even if
  that source equals the snapshot. Backup names include kind, realm, and script
  name plus a collision-safe suffix; the actual path is printed. A backup file
  is created exclusively at mode 0600 on Unix. A backup failure aborts before
  local source or snapshot changes. Direct `script pull --force=backup` is the
  explicit backup opt-out; bare `--force` authorizes overwriting protected edits
  but still creates the backup. Sync never opts out.

  Direct pull preflights all selected scripts. Missing local source, content
  already equal to the tenant, and content equal to a valid snapshot are safe.
  Any other differing source is protected. A single pull offers one default-no
  confirmation; a namespace/`all` pull refuses the whole batch without bare
  `--force` and lists every affected ref before changing a file. The Scripts TUI
  uses the same preflight and one default-cancel modal for the complete
  selection; acceptance always keeps backups.

- **Every write is syntax-checked first.** Before `create`, `copy`, `push`,
  `sync` and `watch` write anything, the tenant is asked to parse the source —
  AM through `scripts?_action=validate`, IDM through `script?_action=compile`.
  Neither write path does this itself: a `PUT` of a script that does not parse
  succeeds with a 201, and the breakage then surfaces far from the edit. A
  broken AM script fails whenever its journey or token flow next evaluates; a
  broken IDM endpoint answers **404** at its runtime URL while its config object
  still reads back 200, so it presents as an endpoint that was never created.

  A refusal writes nothing, leaves the snapshot alone, and exits non-zero, so
  the local edit survives for you to fix and push again. That includes a batch:
  `push all` and `sync` finish the run and print their summary, then exit
  non-zero if anything was refused. AM reports the line and column; **IDM
  reports neither for JavaScript** — its message is the bare parser string
  ("syntax error") even when the fault is on line 40 — and the output says so
  rather than leave you hunting for a coordinate that was never sent.

  **No verdict is also a refusal, with no exceptions.** If the check cannot
  answer — an unexpected body, or a 503 from IDM's compile action, which is what
  it returns for a script `type` it does not recognise _and_ what an unwell
  service returns — nothing is written, and the message says the check gave no
  verdict rather than that the source was rejected. Retrying is the first thing
  to try, since it may be a bad minute on the tenant.

  The same goes for source nothing _can_ check, such as an endpoint declaring a
  script engine the compile action does not compile: that is recognised before
  the call is made, so the message names the type and skips the useless retry
  advice — but it still writes nothing, because unparsed source is unparsed
  source however the gate found out. `--force=syntax-check` is the one way to
  store source the tenant has not parsed.

  `--force` does **not** override this, and that is deliberate: drift is a
  question of whose content wins, while an unparseable script is broken whoever
  wrote it. `--force=syntax-check` is the escape hatch, on every one of those
  commands, for when the check itself is in the way. The pre-flight is one extra
  call, ~0.15s.

  `create`, `copy`, and `watch` support only this named guard and reject bare
  `--force`. `push` and `sync` can combine it with their operation permission,
  for example `aic script push endpoint/example --force --force=syntax-check`.
  See [Flag migration](#flag-migration) for the removed `--no-syntax-check`
  spelling and for the change in what `pull --force` means.

- **A generated bundle is pushed only to the script it was built for.** When
  an endpoint's or managed hook's source opens with the
  `// @generated by \`npm run build\`` banner, the file named on its
  `// Source:` line says what it belongs to: `…/endpoints/<name>.ts` is
  `endpoint/<name>`, and `…/lifecycle/<object>.<hook>.ts` (the layout of the
  outside project that builds hook bundles) is `managed/<object>.<hook>`. For an
  endpoint, a file in any other directory still names `endpoint/<stem>`; for a
  hook, it names nothing. Every write path (`push`, `sync`, `watch`, `create`,
  `copy`, the TUI) refuses a bundle claiming another script before the syntax
  check. No flag overrides this, `--force=syntax-check` included, because the
  source may parse perfectly and still belong to another script. Rebuild, push
  the script the banner names, or delete the banner to push the file as
  hand-written. Source without the banner is not checked.

- **A marked script is pushed only to the script it is marked for.** A build
  outside this repo (hook bundles come from another project) can write
  `// aic-target: <full name>` — the name `aic script push` takes, such as
  `managed/alpha_user.onCreate`, `endpoint/users` or `alpha/<script>` — in the
  source's leading run of `//` comment lines. Every write path refuses a source
  whose marker names another script, or whose markers disagree, before the
  syntax check and past every `--force`. Only the leading block counts, because
  a bundle inlines other modules and their markers belong to them; a leading
  byte-order mark and any spacing after `//` are allowed, so a marker is not
  missed. The source is read lossily, so a byte that is not UTF-8 elsewhere in
the file does not hide the marker; a payload that cannot be decoded at all is
refused. Unmarked source is not checked, so this protects a hook only once its
  producer writes the marker. Delete the marker to push the file unmarked.

- **AM JavaScript that calls a member the AM engine lacks is refused.** After
  the tenant parses an AM script, the same pre-push check scans it for calls to
  `Object.values`, `Object.entries`, `Object.getOwnPropertyDescriptors` and the
  ES2015 globals (`new Map`, `new Set`, `Symbol(…)`, `Promise`, `Reflect`, …),
  each measured absent on AM (`docs/api/12-script-bindings-matrix.md`). They
  parse, so the tenant's check passes them, and they throw when the line runs.
  The refusal gives line, column and the substitute. Feature detection
  (`typeof Object.values`) and a name the source declares itself are not
  flagged; comments and strings are skipped. `--force=syntax-check` writes
  anyway. IDM scripts are not scanned: that engine has `Object.values` and
  `Object.entries`.

- **`status` filters.** `am`/`idm` are group aliases; anything else is a
  case-insensitive substring of the full-name (use a trailing slash, e.g.
  `alpha/`, to match only that AM realm and exclude `managed/alpha_user…`).
- **`status` reports workspace drift** after the script rows. It prints the same
  templates-version nudge that `push`/`pull` already print when the scaffold
  predates the bundled templates. Independently, it fetches the live managed
  schema and regenerates the expected type files in memory (nothing is written):
  if any on-disk managed type file is missing or differs, one line names how
  many are stale and points at `aic workspace update`. Both notes stay silent
  when current. A failed fetch warns once and does not fail the command — the
  script rows still printed.
- **`diff`** shells out to `git diff --no-index` (needs `git` on PATH): colored
  via your pager interactively, plain unified diff when piped
  (`aic script diff bravo/Foo | delta`). Default compares local vs tenant;
  `--local-vs-snapshot` shows your edits since the last pull,
  `--snapshot-vs-remote` shows tenant drift since you pulled.
  `--find-in-git REPO` answers "which branch heads hold these exact bytes?"
  instead of diffing: it hashes the tenant's bytes as a git blob and lists
  every local and remote-tracking branch head in `REPO` whose whole tree holds
  that blob, with the path. Identical bytes are not provenance — several
  branches can hold the same file. Exact bytes only — a rebuild that differs
  by a newline is not a match — and nothing is fetched: fetch first for the
  latest heads. A partial clone missing a tree fails rather than fetching it
  (this needs git 2.45 or later, for `--no-lazy-fetch`; an older git refuses
  the search instead of risking a fetch), `git replace` objects are ignored, and an inherited `GIT_DIR` does not
  redirect the search. A script you have not pulled is looked up on the
  tenant by exact name.

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
