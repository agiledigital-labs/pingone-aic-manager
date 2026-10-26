# 23 — Promotion (lock state, status, reports)

Verified against a dev → UAT environment pair and a sandbox, 2026-10-26, with
`scripts/verify-endpoint.sh` and then `aic promotion`. All reads; nothing here
has locked, promoted or unlocked anything.

`aic promotion status|reports|report|verify` (`src/promotion/`) wraps the reads
below. There is deliberately no write verb yet.

## Scope

Every path needs `fr:idc:promotion:*` on the token. Without it:

```
403 {"code":403,"message":"The access token does not have sufficient scope"}
```

That is a permissions answer, not a lock state. The scope has to be granted on
the tenant's service account (console) **and** requested by aic — see
`00-auth.md` ("Service-account scopes") and `aic ctx scopes`.

## Endpoints

| Method | Path                                         | Observed                                                                                       |
| ------ | -------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| GET    | `/environment/promotion/lock/state`          | 200. Upper env: `{description, lowerEnv{proxyState,state}, result, upperEnv{…}}`. Lower env: `{description, result}` only |
| GET    | `/environment/promotion/promote`             | 200 `{blockingError, globalLock, message, status, timeStamp, type?}`                           |
| GET    | `/environment/promotion/status`              | **501** — the status lives on `GET …/promote`                                                  |
| GET    | `/environment/promotion/lock`                | **405** — `POST` only (not exercised)                                                          |
| GET    | `/environment/promotion/reports`             | 200, array of `{createdDate, dryRun, promoter, promotionId, reportId, type}`; `[]` on the lower env |
| GET    | `/environment/promotion/report`              | 200 latest report on the upper env; lower env **404** `{"message":"No latest report data found"}` |
| GET    | `/environment/promotion/report/{reportId}`   | 200, one report                                                                                |
| GET    | `/environment/promotion/report/provisional`  | 200 on the upper env, generated on request **with no lock held**                               |
| GET    | `/environment/promotion/provisional-report`  | 404 — not a path                                                                               |
| GET    | each GET above, on a sandbox                 | **503** HTML "No server is available to handle this request" — sandboxes have no promotion service |

Measured reads answer on the **upper** environment: only it reported both
sides' lock, held the reports, and answered the provisional report. Which
environment a lock or promotion is *started* from is not measured.

## Report shape

```json
{
  "createdDate": "2026-10-26T00:00:00Z",
  "promoter": "<promoter>",
  "promotionDescription": "<optional>",
  "promotionId": "<uuid>",
  "reportId": "<uuid>",
  "reportName": "<name>",
  "type": "promotion",
  "report": {
    "AMConfig": [
      { "configItem": "Journeys",
        "configChange": { "added": [{ "name": "<tree>", "realm": "/alpha" }],
                          "modified": [{ "name": "<tree>", "realm": "/alpha", "uid": "<optional>" }] } }
    ],
    "IDMConfig": [
      { "configItem": "IDM Themes By Realm", "configChange": { "modified": [{ "name": "<theme>", "realm": "alpha" }] } }
    ]
  }
}
```

`configChange` keys seen: `added`, `modified`, `deleted`. `realm` is absent on
global items (e.g. "Global Scripts"). `configItem` values seen include Journeys,
Scripts, Global Scripts, CORS, "ESV Secrets Store Mappings", "AM Services >
…", "AM Authentication > Settings", "IDM Themes By Realm". `aic promotion
report` refuses a report it cannot count rather than summarise part of it.
An empty `report` prints "no changes listed", not "no differences": no
empty-report control has been measured to say it means a clean comparison.

## The two lock descriptions can disagree

Measured on the upper environment, 2026-10-26: `lock/state` said `unlocked`
for `result` and both sides, while `GET …/promote` said `status: UNLOCKING`,
`message: "Environment unlock in progress"`, `globalLock: UNLOCKED`.
`aic promotion status` prints both and exits non-zero naming each difference,
rather than choosing one, with and without `--json`. What is measured is that
they **can** differ — not which fields must always agree (`UNLOCKING` may be a
transition, and the two reads are sequential) — so a difference is an
indeterminate observation, not a diagnosis. Case alone is not a difference. A
reported `blockingError: true` also exits non-zero, on its own. Agreeing
`LOCKED` values exit zero: this is not a readiness check. Which source a lock
request consults is unmeasured.

## Not yet exercised

Locking, unlocking, dry runs and promotion (`POST` on `lock`, `promote`).

## The provisional report is not a live diff

Measured 2026-10-26 on a dev → UAT pair the console called in sync. The
provisional report (regenerated twice, 04:56 and 05:52 UTC) listed **150**
items. Each sampled item was fetched from both tenants with
`scripts/verify-endpoint.sh --raw`, after a control proving the two calls
reached two different hosts (dev answered `lock/state` with the lower-env
shape, UAT with the upper):

| Reported as                         | Sampled                                   | Live result                                             |
| ----------------------------------- | ----------------------------------------- | ------------------------------------------------------- |
| Journeys / added (7)                | one bravo journey                         | present on UAT, byte-identical, same `_rev`             |
| Journeys / modified (63)            | one alpha journey                         | byte-identical, same `_rev`                             |
| Scripts / added (3)                 | one bravo script                          | present on UAT, byte-identical                          |
| Scripts / modified (15)             | one alpha script                          | byte-identical; `lastModifiedDate` seven months earlier, before the last promotion |
| ESV Secrets Store Mappings / added (48) | the 42 alpha entries                  | all 42 already mapped on UAT; the alpha mapping list is byte-identical |

AM services, CORS, the global script and the IDM theme were not sampled.

Nine of the 48 mapping "adds" were also in the last real promotion's report
(four days earlier). One possibility is that the service does not learn from
a promotion for that item type; this was not tested further. The other 141
items were not in that report.

So the provisional report's items are the promotion service's claim, and they
do not reflect a comparison of the live content: why is not known — it could
compare against something other than the live upper environment, or list items
by enumeration or a stale classification; these observations do not separate
those. Never read it as "dev and UAT differ". Unexplained as of this date:
what it compares against — a stored baseline is one possibility, not an
observation; whether the upper env's `UNLOCKING` status (unchanged since
that promotion) is related.

## Verifying a report

`aic promotion verify --lower <env> --upper <env> [--report <id>]` reads the
report from the upper environment (provisional by default), fetches every item
from **both** tenants and compares the content with copy metadata removed **at
the resource root only**: `_rev` from every AM document (for a journey, from
the tree and from each node), plus `createdBy`, `creationDate`,
`lastModifiedBy` and `lastModifiedDate` from a script. A field spelt the same
deeper inside a configuration is compared. The report's change kind is not
trusted: an item is judged on what each tenant returns.

Each report item maps to one live read, all measured 2026-10-26 on the dev →
UAT pair (200 on both unless stated):

| Report item                           | Read                                                                                                   |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `Journeys`                            | `…/realms/{realm}/realm-config/authentication/authenticationtrees/trees/{name}` and every node the tree lists, page-node children included; an inner-tree node's target journey is not followed (it is its own item) |
| `Scripts` with a `uid`                | `…/realms/{realm}/scripts/{id}` (`resource=1.1`); the id is the first `ou=` of `uid`                    |
| `Scripts` without a `uid`             | `…/realms/{realm}/scripts?_queryFilter=name eq "<name>"` — the report gave one entry no `uid`           |
| `ESV Secrets Store Mappings`          | the secret mapping for the entry's name, which is the AM secret label (404 = unmapped)                 |
| `AM Services > <name>`                | `…/realms/{realm}/realm-config/services?_queryFilter=true`, then `…/services/{_id}`                    |
| `AM Authentication > Settings`        | `…/realms/{realm}/realm-config/authentication`                                                         |
| `CORS`                                | `/am/json/global-config/services/CorsService/configuration?_queryFilter=true`, matched by `_id`        |
| `IDM Themes By Realm`                 | `/openidm/config/ui/themerealm`, the realm's **whole** list                                            |
| `Global Scripts`                      | no reader: `/am/json/realms/root/scripts` is 403 and `global-config/services/scripting/globalScripts` 404 |

AM config reads use `Accept-API-Version: protocol=1.0,resource=1.0`.

Three naming mismatches, each measured, decide the lookups:

- **Service names** — the item says `Session Service` where the service list
  says `Session`, and `OAuth2 Provider Service` where it says
  `OAuth2 Provider`; `Push Notification Service` and `Validation Service` match
  as given. The name is tried as given and without a trailing ` Service`.
- **CORS ids are lowercased** in the report (`client-a` for `Client-A`), so the
  configuration is matched case-insensitively against the list. Two matches
  are an error, not a pick.
- **The theme name does not match the stored one** (`Starter` against
  `Starter Theme`), so the realm's whole theme list is compared instead of
  guessing which theme is meant.

An earlier draft looked CORS entries and the theme up by the report's name,
found them on neither tenant, and called four items in sync. So **absent from
both counts as in sync only for an item reported `deleted`**; for one reported
added or modified it is unchecked, because it means the lookup did not find
what the report meant. A read error on either side, including one journey node,
is likewise unchecked, never a match — and so is a response that is not the
shape measured: a tree or `PageNode` without its `nodes`, a theme document
without a `realm` object, a list whose `pagedResultsCookie` is set or which
reports remaining results. An empty theme list is absence, so it too is
unchecked unless the item was reported deleted. A script without its `script`
source, a theme entry that is not an object, and a CREST error envelope (an
HTTP-error `code` beside a string `reason` or `message`) answered in place of
a document are unchecked as well. `verify` exits non-zero when anything
differs or is unchecked.

**Not measured: that the service and CORS lists are complete.** Both are read
with `_queryFilter=true` and no paging request, and neither carried a
`pagedResultsCookie` or remaining results on the pair measured. That rules out
the paging this checks for; it does not prove AM never truncates an unpaged
list. A truncated list would make a present item look absent — unchecked, if
it was reported added or modified, never a false `same`.

Measured on the pair the console called in sync: **149 of 150 same, 0 differ,
1 unchecked** (the global script). Positive control, the same report with the
lower side swapped for a third environment: 73 same, 76 differ (18 `DIFFERS`,
58 present only on the upper side), 1 unchecked — the comparison does find
differences when they exist.

