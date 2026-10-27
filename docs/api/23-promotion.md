# 23 — Promotion (lock state, status, reports)

Verified against a dev → UAT environment pair and a sandbox, 2026-10-26, with
`scripts/verify-endpoint.sh` and then `aic promotion`. The write cycle — lock,
dry-run promotion, unlock — was measured on the same pair on 2026-10-27, first
with `curl` and then end to end through `aic promotion lock` / `dry-run` /
`unlock`. A real promotion was measured the same day as a round trip through
`aic`: a temporary IDM endpoint created on dev, locked, dry-run, promoted,
verified and unlocked; then deleted on dev and the deletion promoted the same
way (see "The real round trip" below).

`aic promotion status|reports|report|verify` (`src/promotion/`) wraps the reads
below, and `aic promotion lock|dry-run|promote|unlock` the write cycle
(`src/promotion/lifecycle.rs` holds the states each may start from).

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
| GET    | `/environment/promotion/lock`                | **405** — `POST` only                                                                          |
| POST   | `/environment/promotion/lock`                | 202 `{description: "Environment lock in progress", promotionId, result: "locking"}` (no body sent) |
| POST   | `/environment/promotion/promote`             | 202 `{result: "Promotion process initiated successfully"}` with body `{"dryRun":true}`        |
| DELETE | `/environment/promotion/lock/{promotionId}`  | 202 `{description: "Environment unlock in progress", promotionId, result: "unlocking"}`       |
| GET    | `/environment/promotion/reports`             | 200, array of `{createdDate, dryRun, promoter, promotionId, reportId, type}`; `[]` on the lower env |
| GET    | `/environment/promotion/report`              | 200 latest report on the upper env; lower env **404** `{"message":"No latest report data found"}` |
| GET    | `/environment/promotion/report/{reportId}`   | 200, one report                                                                                |
| GET    | `/environment/promotion/report/provisional`  | 200 on the upper env, generated on request **with no lock held**                               |
| GET    | `/environment/promotion/provisional-report`  | 404 — not a path                                                                               |
| GET    | each GET above, on a sandbox                 | **503** HTML "No server is available to handle this request" — sandboxes have no promotion service |

Measured reads answer on the **upper** environment: only it reported both
sides' lock, held the reports, and answered the provisional report. The
write cycle below was started from the upper environment too; starting any of
it from the lower one has not been tried.

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

## The write cycle

Measured 2026-10-27 on the dev → UAT pair, twice for lock/unlock and once for
the dry run, every call sent to the **upper** environment. Starting a lock from
the lower environment was not tried.

| Step | Request | Settles as | Took |
| ---- | ------- | ---------- | ---- |
| Lock | `POST …/lock` | `lock/state` `result: locked`, both sides `locked` (the upper side first, ~15 s earlier); `GET …/promote` then `status: READY`, `message: "Environment ready for promotion"`, `globalLock: LOCKED`, and carries the `promotionId` | ~3¼ min |
| Dry run | `POST …/promote`, `Content-Type: application/json`, `{"dryRun":true}` | `GET …/promote` `status: RUNNING` through the messages `Start promotion`, `Clone repos`, `Checking promotion can proceed`, `Create report`, `Resetting promotion-gate attributes`; then `status: READY`, `message: "Promotion completed (no change)"`, still `globalLock: LOCKED` | ~8½ min |
| Promote | `POST …/promote`, `Content-Type: application/json`, `{"dryRun":false}` | `RUNNING` through `Clone repos`, `Checking promotion can proceed`, `Update configuration`, `Tag lower env`, `Restart workloads`, `Resetting promotion-gate attributes`; then `status: READY`, `message: "Promotion completed"`, still `globalLock: LOCKED` (measured twice, through `aic`) | not timed; well inside 45 min |
| Unlock | `DELETE …/lock/{promotionId}` (the id the lock returned) | `lock/state` `unlocking` on both sides together, then `unlocked`; `GET …/promote` `status: UNLOCKED` | ~3¼ min |

What the dry run left behind: one new `reports` entry with `dryRun: true` and
**no `promoter` key** (every real promotion's entry carries one), whose
`reportName` ends `_dryrun=true_<promotionId>`, and the environment still
locked — a dry run does not release the lock.

A dry run ends `READY`, the same status a lock settles at and a promotion
starts from, so `READY` alone does not say a run finished; `aic` takes it as
the end only after seeing `RUNNING` under the same `promotionId`. A report is
not taken as evidence the run ended: the status named a "Create report" step
before later steps, and when the report becomes readable has not been
measured. A real promotion ends the same way — `READY`, still locked — measured
twice. **"(no change)" is not a dry-run marker**: it appeared only on the dry
run of the pair with nothing to promote, while both dry runs that had something
to promote ended "Promotion completed", as both real promotions did.

**The observed step messages differed between runs.** The three dry runs
showed three sets: `Start promotion`, `Clone repos`, `Checking promotion can
proceed`, `Create report`, `Resetting promotion-gate attributes` (nothing to
promote); the same without `Start promotion` (an endpoint added); and `Start
promotion`, `Clone repos`, `Checking promotion can proceed`, `Notifying that
config has been pushed`, `Resetting promotion-gate attributes` (an endpoint
deleted) — with no `Create report`, yet that run's report was there afterwards.
The messages are sampled by polling, so a short step can go unseen, and one
fixed sequence with differently timed steps would produce the same samples.
`aic` prints them and decides nothing on them.

**On the pair with nothing to promote, the report repeated the previous one.** That dry run's
report was byte-identical to the report of the last real promotion five days
earlier — a global script `modified` and nine ESV secret mappings `added` —
while the service's own message said "no change" and the console showed
nothing to promote. The global-script item was also in both earlier real reports
checked (2026-10-11 and 2026-10-22). So in that case a report is not evidence of what a promotion changed; this is
the same observation as the provisional report below, made on a completed run.

**With something to promote, the reports listed only that.** All four reports of
the real round trip — two dry runs, two promotions — named the one endpoint and
nothing else; the global script and the nine mappings did not appear. Four runs
on one pair do not establish that a report always lists exactly the change, so
`aic promotion verify` remains the live comparison either way.

Not exercised: other `promote` bodies, a
`POST …/promote` with no body, unlocking with a stale or foreign
`promotionId`, a second lock while one is held, a promotion while unlocked,
and any failed promotion — so which states a failure leaves (and so what
`aic promotion unlock --recover` will meet) is unknown.

The first lock `POST` was sent with no body. `aic`'s transport sends JSON
`null` as the body of a bodyless `POST`, as it does for every `DELETE`, and
the cycle run through `aic` on 2026-10-27 measured both: `POST …/lock` with a
`null` body answered 202 "Environment lock in progress" and settled locked;
`DELETE …/lock/{promotionId}` with a `null` body answered 202 and settled
unlocked.

That run also observed:

- **The status carries the lock's `promotionId` during the transitions.**
  `LOCKING` and `UNLOCKING` both named the same id the lock acknowledgement
  returned, and so did the settled `READY`.
- **The proxies move first.** Mid-lock each side read state `locking` with
  proxy `locked`; mid-unlock, state `unlocking` with proxy `unlocked`. A
  reading whose proxy disagrees with its state is a transition, which is why
  `aic` never starts a write from one.
- **A status read can fail transiently mid-lock**: one `GET …/promote` answered
  `500` "unable to read promotion-status document … context deadline exceeded";
  the next read succeeded. `aic` gives up waiting at six consecutive failed
  reads, or at the timeout — whichever comes first.
- **The dry run's report was found by difference** from the list read before
  it, as the only new `dryRun: true` entry under the lock's `promotionId`, and
  the run ended "Promotion completed (no change)" after `RUNNING` was seen.

## The real round trip

Run 2026-10-27 through `aic` on the dev → UAT pair, every write sent to UAT. A
temporary endpoint `endpoint/aic-promotion-probe` was created on dev with `aic
script create`; then `lock`, `dry-run`, `promote`, `verify`, `unlock`. Then the
endpoint was deleted on dev with `aic script delete` and the same cycle run
again. The plan was to stop before promoting if either dry run's report listed
anything beyond the probe; neither did.

| Run | Report | Item | `verify` |
| --- | ------ | ---- | -------- |
| Dry run, then promote (one lock) | `IDMConfig` / `IDM Custom Endpoints` | `endpoint-aic-promotion-probe`, `added` | `same` |
| Dry run, then promote (one lock) | `IDMConfig` / `IDM Custom Endpoints` | `endpoint-aic-promotion-probe`, `deleted` | `absent-both` (in sync: reported deleted) |

Afterwards `GET /openidm/config/endpoint/aic-promotion-probe` answered 200 on
UAT, identical to dev, after the first promotion, and 404 on both after the
second.

- **A real promotion's report** has `dryRun: false` and a `reportName` ending
  `_dryrun=false_<promotionId>`. The dry run and the promotion inside one lock
  shared the lock's `promotionId`, and each report was found by difference from
  the list read before its run.
- **A report names an endpoint by its config id with `/` as `-`**:
  `endpoint-aic-promotion-probe` for `endpoint/aic-promotion-probe`.
- **Two consecutive status reads failed mid-lock** with 500 "context deadline
  exceeded" (once from the lower-lock check, once fetching the promotion
  status); the third succeeded. The limit was then three, the third failure
  ending the wait, so that lock came within one read of giving up waiting —
  the lock itself settled normally. The limit is now six.

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
| `IDM Custom Endpoints`                | `/openidm/config` (the config list), then `/openidm/config/{_id}` for the one `factoryPid: "endpoint"` entry the name stands for; 404 = absent. Measured 2026-10-27: `{_id, source, type}`, no `_rev`, compared whole; the returned `_id` must be exactly the id read, and only that inline shape is compared (a `file`-backed endpoint is unchecked) |
| `Global Scripts`                      | no reader: `/am/json/realms/root/scripts` is 403 and `global-config/services/scripting/globalScripts` 404 |

AM config reads use `Accept-API-Version: protocol=1.0,resource=1.0`.

Four naming mismatches, each measured, decide the lookups:

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
- **An endpoint's `/` becomes `-`** (`endpoint-aic-promotion-probe`). The
  config read API takes two-part ids only: `GET
  /openidm/config/endpoint/<a>/<b>` is a 400 "The passed resourcePath has more
  than two parts" on both tenants, even under an endpoint that exists, and a
  404 for an absent two-part id. So `verify` supports `endpoint/<name>` ids
  only — `endpoint-a-b` is read as `endpoint/a-b` — and rejects a deeper id in
  the list. That no stored id has more parts is not shown: the 400 may come
  from routing, before any lookup. The name is still matched against the
  config list's endpoint ids encoded the same way. Two candidates are an error, and so is one that
  matches only without case: the report lowercases CORS ids, but whether it
  folds endpoint names is not measured (no report on the pair had ever listed
  an endpoint before this round trip). A wrong guess would be fail-open for a
  deletion, where absent from both is in sync — so a paged list, a row with no
  `_id`, and a row whose `factoryPid` and `endpoint/` prefix disagree are
  errors too, never "not found". On both tenants, 2026-10-27, the list had 97
  rows, no paging keys, every row an `_id`, and the two signs agreeing.

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

The IDM config list carries the same caveat, and there absence matters more:
an endpoint reported **deleted** and missing from both lists is in sync. So
absence is never taken from the list. An endpoint the list does not name is
read directly at the one id its name stands for, and is absent only if that
answers 404. A 200 means the list omitted it, and any other answer is an
error; both make the item unchecked, as does a name not of the form
`endpoint-<name>`. What the direct read cannot exclude is a different *case*: if reports fold endpoint
names as they fold CORS ids, an omitted `endpoint/Foo` reported as
`endpoint-foo` is excluded only by the list.

Measured on the pair the console called in sync: **149 of 150 same, 0 differ,
1 unchecked** (the global script). Positive control, the same report with the
lower side swapped for a third environment: 73 same, 76 differ (18 `DIFFERS`,
58 present only on the upper side), 1 unchecked — the comparison does find
differences when they exist.

