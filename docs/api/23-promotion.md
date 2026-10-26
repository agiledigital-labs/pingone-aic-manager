# 23 — Promotion (lock state, status, reports)

Verified against a dev → UAT environment pair and a sandbox, 2026-10-26, with
`scripts/verify-endpoint.sh` and then `aic promotion`. All reads; nothing here
has locked, promoted or unlocked anything.

`aic promotion status|reports|report` (`src/promotion/`) wraps the reads below.
There is deliberately no write verb yet.

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

Locking, unlocking, dry runs and promotion (`POST` on `lock`, `promote`). The
provisional report means a diff does not need a lock.
