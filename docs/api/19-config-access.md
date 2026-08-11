# 19 — `config/access`: IDM authorization rules

Implemented in: [`src/access/`](../../src/access/) (`aic access`).

## Purpose

`config/access` is the rule list that gates `/openidm` routes **for the
identities it governs** — which is not all of them, and not all routes for a
given identity; see
[the scoped bypass](#the-service-account-bearer-is-not-fully-governed-by-configaccess)
below before concluding that a rule here controls a request. It is the single
object most likely to lock every operator out of a tenant, which is why this
file exists separately from [18-internal-roles.md](18-internal-roles.md): the
roles are the _subjects_, and this document is the _policy_ that refers to them.

Read [18-internal-roles.md](18-internal-roles.md) for how a role reference is
spelled — including why the reference must be the role's `_id` and never its
`name`, and why a reference to a role that does not exist as an object is normal
practice rather than a defect.

## Authentication

Service-account bearer, scope `fr:idm:*`. **Tenant-global — no realm segment**,
like ESVs and IDM config generally.

## Endpoints

| Op      | Method | Path                     | Notes                                               |
| ------- | ------ | ------------------------ | --------------------------------------------------- |
| Read    | `GET`  | `/openidm/config/access` | 200, `{_id, configs[]}`.                            |
| Replace | `PUT`  | `/openidm/config/access` | Whole-document replace. `_id` may stay in the body. |

There is no per-rule endpoint. Every change is a read-modify-write of the whole
document.

## Object shape

Top-level keys are exactly `_id` and `configs`. Each `configs` entry is a rule:

```json
{
  "actions": "*",
  "excludePatterns": "repo,repo/*,file/iwa/*",
  "methods": "*",
  "pattern": "*",
  "roles": "internal/role/openidm-admin"
}
```

Key frequency over the sandbox's 65 rules (2026-08-11):

| Key               | Present in  | Notes                                                          |
| ----------------- | ----------- | -------------------------------------------------------------- |
| `pattern`         | 65 / 65     | The route glob. `*` matches everything.                        |
| `roles`           | 65 / 65     | **Comma-separated string**, or `*`. No sandbox rule lists two. |
| `methods`         | 65 / 65     | Comma-separated. Observed vocabulary below.                    |
| `actions`         | **59 / 65** | **Optional — six live rules omit the key entirely.**           |
| `customAuthz`     | 22 / 65     | A JS expression that can only _deny_; see below.               |
| `excludePatterns` | 1 / 65      | Comma-separated globs. **Semantics inferred, not observed.**   |

**`actions` being optional is a tooling trap.** Anything that round-trips a rule
through typed fields and re-serialises it will hand those six rules an `actions`
key they never had, silently rewriting rules it was not asked to touch. Mutate
the parsed JSON in place instead of rebuilding it.

Methods observed in the wild: `read`, `query`, `create`, `update`, `delete`,
`patch`, `action`, `script`, `*`. There is **no published enum**, so an
unrecognised method is a reason to warn, not to refuse.

The only `excludePatterns` rule on the sandbox is rule 17
(`repo,repo/*,file/iwa/*` under `pattern: "*"` for `openidm-admin`). That it
_subtracts_ from `pattern` is the obvious reading, but **no probe has exercised
it** — an attempt to use `file/iwa/*` as a denial probe returned 404 from
routing before authorization, so it could not serve as one. Treat the semantics
as unverified.

`roles` here is a comma-separated string; the same conceptual field in
`config/authentication` (`rsFilter.staticUserMapping[].roles`) is an **array**.
Anything that writes both must know the difference —
[18-internal-roles.md](18-internal-roles.md) has the table.

## No `_rev` — content is the only precondition

`config/access` carries **no `_rev` at all**, unlike internal roles. There is no
conditional write to use, so the only precondition available is a content
comparison against the document as previously read (`CLAUDE.md` §5), and a
backup is the only safety net.

## `config/access` survives a read-modify-write

Relevant because it is the object most likely to lock an operator out. Verified
2026-08-10 on a 65-rule tenant: `GET`, append one rule, `PUT` the whole object
back, and all 65 pre-existing rules return **byte-identical and in original
order**, with no top-level keys lost. Restoring the saved original is likewise
byte-exact. The `_id` may be left in the `PUT` body.

So read-modify-write `PUT` is safe to build on. **Back the object up first
anyway** — the failure mode is losing authorization for everyone.

## `configs` is a disjunction, not first-match-wins (resolved 2026-08-10)

**A request is permitted if _any_ rule grants it.** A rule that matches the
pattern but does not grant — wrong `roles`, or a `customAuthz` that returns
false — does **not** terminate evaluation. So a rule appended to the end of
`configs` can never be shadowed by an earlier, broader rule, and **tooling may
append**.

Measured against a probe that the tenant's 65 existing rules deny:
`GET /openidm/managed/svcacct?_queryFilter=true&_fields=_id` → **403**. The
grant under test was
`{"pattern":"managed/svcacct","roles":"internal/role/openidm-authorized","methods":"query","actions":"*"}`.
Every injected rule used `pattern: "managed/svcacct"` exactly, and every `PUT`
body was rebuilt from the untouched 65-rule original, so no case could
accumulate on another.

| `configs` sent                                                     | Probe   |
| ------------------------------------------------------------------ | ------- |
| original (65) — baseline                                           | 403     |
| original + grant (66)                                              | **200** |
| non-matching-role rule first, original, grant last (67)            | 200     |
| grant first, original, non-matching-role rule last (67)            | 200     |
| original + `customAuthz: "false"` rule + grant (67)                | 200     |
| original + `customAuthz: "false"` rule only (66)                   | **403** |
| original + `customAuthz: "(function(){return false})()"` only (66) | **403** |

The last two rows are what make the fifth interpretable. A `customAuthz` of
`false` really does deny on its own, so row five is evaluation **continuing
past** a rule that matched pattern, roles and methods and then refused — not the
refusal being ignored. The baseline agrees independently: rules 37 and 38 are
`pattern: "*"` for `internal/role/openidm-authorized` with `customAuthz`
`ownDataOnly()`, which match this identity and fail for this query, and an
appended grant still took effect past them.

Consequences for tooling:

- **Append; no insertion-position logic is needed.** The feared silent no-op —
  an appended grant shadowed by an earlier broader pattern — does not occur.
- **A new rule cannot revoke anything.** There are no deny rules, only grants
  that may decline. Narrowing existing access means **editing or removing the
  rule that grants it**; appending a "restriction" is the real silent no-op
  here, the inverse of the failure that was feared. So `edit` and `rm` are the
  dangerous verbs and the ones that need a diff and a confirmation.
- **Changes take effect immediately.** Both 403→200 transitions were visible on
  the first probe after the `PUT`; no retry was ever needed, and no propagation
  delay was observed.

The answer is only as general as the identity it was measured with: a
service-account bearer whose `GET /openidm/info/login` roles are
`internal/role/openidm-svcacct` and `internal/role/openidm-authorized`
(component `managed/svcacct`). Behaviour for an `openidm-admin` caller or an
anonymous one was not measured.

## The service-account bearer is not fully governed by `config/access`

Verified 2026-08-10, and it constrains how authz tooling can be built.

`GET /openidm/info/login` for our service-account bearer reports roles
`internal/role/openidm-svcacct` + `internal/role/openidm-authorized`, component
`managed/svcacct`. Two things follow from the live config:

- **`openidm-svcacct` does not exist** as an object
  (`GET /openidm/internal/role/openidm-svcacct` → 404) and the string appears in
  **no rule** in `config/access`. It is a synthetic role — the legitimate case
  described in [18-internal-roles.md](18-internal-roles.md).
- **Nothing in `config/access` grants these roles `query` or `create` on
  `internal/role`.** No rule with either method has a pattern covering it; rule
  25 grants `read,query` on `internal/role/*` to `platform-provisioning` only,
  and the two `pattern: "*"` rules for `openidm-authorized` (37, 38) list
  `read,action,delete` and `update,patch,action` — neither includes `query` or
  `create`.

Yet `GET /openidm/internal/role?_queryFilter=true` returns **200** and
`PUT /openidm/internal/role/{new-id}` returns **201**. Reads of `config/access`
and `config/authentication` likewise return 200. So on those paths the bearer is
not evaluated against `config/access` at all.

It is not a blanket bypass: `GET /openidm/managed/svcacct?_queryFilter=true`
returns **403**, and adding a rule for that path flips it to 200 (see the case
table above). So the filter is live for this identity on some paths and absent
on others. **The mechanism has not been established** — scope-based
authorization for service accounts is a plausible explanation but is not
verified here, and nothing should be built on it.

Two consequences for a guarded-write feature over this config:

- **`aic` cannot confirm a rule change by trying the operation itself.** Its own
  access may not be governed by the rule it just wrote, so an empirical "did it
  work?" check would silently prove nothing. Validation has to be structural —
  check the shape, resolve every role reference, show the operator a diff —
  rather than "write it and probe".
- **A bad write cannot lock the tool out of repairing it**, because its access
  to `config/access` does not come from `config/access`. That is a real safety
  property for the feature, but it is an observation about this tenant today,
  not a guarantee — keep taking a backup first.

## Open questions

- **Does `config/access` share managed config's lost-update failure?**
  `99-quirks-and-open-questions.md` Q14 records a verified accepted-but-not-
  persisted `PUT` for the managed-object config document. Nothing equivalent has
  been observed for `config/access` — every experimental `PUT` above persisted
  on the first read-back — but it has not been ruled out either, and the failure
  mode here is worse. A read-back after write is cheap insurance, not a
  workaround for a verified defect.
- **What is the actual bypass mechanism** for the service-account bearer (the
  section above)? Scope-based authorization is a guess.
- **Is `customAuthz` enumerable?** The 22 live values reference helpers
  (`ownDataOnly()`, `checkIfAnyFeatureEnabled('kba')`, …) with no published list
  of what is in scope.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com` (sandbox), no realm segment.

### Object shape and key frequency — 2026-08-11

From a live call made by the author of that section, not transcribed:

- `GET /openidm/config/access` → 200. Top-level keys exactly `_id` and
  `configs`; 65 rules, of which **59 are distinct** — indices 41–47 are seven
  byte-identical copies of one rule. Key counts as tabulated above, i.e.
  `actions` present on 59 of 65. `methods` vocabulary tabulated from the same
  response. No rule carries more than one role. `roles: "*"` is a **real
  observed form**, not an inference — rules 0, 1 and 2 use it (e.g.
  `{"actions":"*","methods":"read","pattern":"info/*","roles":"*"}`), which is
  what makes it legal for the validator to accept.
- `aic access list` against the same document printed a canonical sha256 of
  `75189406f2cad0de785a306176deb50fb57291319015946e98a2ae9e5900cf7f` — equal to
  the value the 2026-08-10 group below computed independently in Python with
  `json.dumps(sort_keys=True, separators=(',',':'))`. Two implementations
  agreeing pins the canonical form the tool's digests depend on, and confirms
  the document is still byte-identical to its pre-experiment state. **This is a
  property of the canonicalisation, not of the API** — if `serde_json` ever
  gains the `preserve_order` feature the agreement breaks silently.

### Read-modify-write survives — 2026-08-10

One bullet from the live run recorded in
[18-internal-roles.md](18-internal-roles.md), made by that file's author while
writing it and moved here with the section it supports:

- `GET /openidm/config/access` → 200, `{_id, configs[65]}`; append-one-rule
  `PUT` → 200; read-back byte-identical for all 65 originals; restore `PUT` →
  200 with a byte-identical final read.

### Evaluation order — 2026-08-10

A separate live run, made by the agent that resolved the evaluation-order
question from its own calls. None of the statuses below were transcribed from a
task prompt or a neighbouring doc.

- Identity, which is the limit of the result's generality:
  `GET /openidm/info/login` → 200 with roles
  `["internal/role/openidm-svcacct","internal/role/openidm-authorized"]` and
  component `managed/svcacct`. `internal/role/openidm-svcacct` does not exist as
  an object (404) and is named in no `config/access` rule.
- Probe `GET /openidm/managed/svcacct?_queryFilter=true&_fields=_id` → **403**
  against the pristine 65-rule object. Nonexistent types
  (`managed/nope_not_real`, `file/iwa/x`) 404 from routing before authz and so
  cannot serve as probes.
- Seven `PUT /openidm/config/access` → 200. Every body was rebuilt from the
  pristine 65-rule object, injecting only rules whose `pattern` was exactly
  `managed/svcacct`; each `PUT` was followed by a re-`GET` asserting the rule
  count and that the 65 originals were intact and in order, then by the probe.
  Grant appended → **200**; non-matching-role rule first with the grant last →
  200; grant first with the non-matching-role rule last → 200;
  `customAuthz: "false"` rule then the grant → 200; `customAuthz: "false"` rule
  alone → **403**; `customAuthz: "(function(){return false})()"` alone →
  **403**; grant alone → **200**.
- Restore `PUT` → 200, and the re-`GET`'s canonical form
  (`json.dumps(obj, sort_keys=True, separators=(',',':'))`) hashed to sha256
  `75189406f2cad0de785a306176deb50fb57291319015946e98a2ae9e5900cf7f`, equal to
  the pre-experiment value; the probe returned to **403**.
  `config/authentication` was never touched.

### Bypass scoping — 2026-08-10

From calls made directly by the author of that section:

- `GET /openidm/internal/role?_queryFilter=true` → **200** and
  `GET /openidm/config/access`, `GET /openidm/config/authentication` → **200**,
  none of which any rule matching `openidm-svcacct` / `openidm-authorized`
  grants. The `create` counterpart (`PUT /openidm/internal/role/{new-id}` → 201)
  is recorded in [18-internal-roles.md](18-internal-roles.md).
- `GET /openidm/managed/svcacct?_queryFilter=true` → **403** on the same bearer
  in the same minute, which is what makes it a scoped bypass rather than a
  blanket one. The mechanism was not determined.
- Rule enumeration supporting the claim was done over the fetched 65-rule
  object, not from memory of the console: no rule pairs `openidm-svcacct` or
  `openidm-authorized` with methods `query` or `create` under a pattern covering
  `internal/role`.
- Independent post-experiment check from a separate process: `config/access` →
  200 with 65 rules, canonical sha256
  `75189406f2cad0de785a306176deb50fb57291319015946e98a2ae9e5900cf7f`, the
  `configs` array equal element-for-element and in order to the pre-experiment
  capture, no `managed/svcacct` rule remaining, and the probe back to **403**.
