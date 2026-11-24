# Journey runtime

## Purpose

What an AM authentication tree (journey) does at runtime, node by node: which
callbacks a node emits, which state keys it reads and writes, what outcomes it
reports, and what `/json/authenticate` returns at each step. This is the ground
truth for the local journey emulator, which must reproduce it. Tree and node
CRUD and the node-type catalog are in `09-journeys.md`; the next-gen
scripted-decision bindings are in `12-script-bindings-matrix.md`; this file is
about _behaviour_.

## Authentication

Config writes use the service-account bearer (`fr:am:*`) with
`Accept-API-Version: protocol=2.0,resource=1.0`. Journeys are driven
anonymously with `POST /am/json/realms/root/realms/alpha/authenticate?authIndexType=service&authIndexValue=<tree>`
and `Accept-API-Version: protocol=1.0,resource=2.1`.

**Never serialise `context`, `requestHeaders` or `existingSession` in a probe,
and never print a `tokenId`.** Read the fields an experiment needs by name.

## Verified against

Sandbox tenant, realm `alpha`, **2026-11-02 to 2026-11-04** and, for "Built-in node types", **2026-11-25**, next-gen scripted decisions
(`evaluatorVersion: "2.0"`). Every section names the
`scripts/experiment-journey-*.sh` run that established it; those scripts
create their own `zzemu-*` trees, nodes, scripts and users and delete them on
exit (set `AIC_REPO` to the checkout whose `.aic/` holds the tenant context).
Anything under "Not yet measured" is not.

## Page node

Measured 2026-11-02 with `scripts/experiment-journey-page.sh`. The page held
`UsernameCollectorNode`, `PasswordCollectorNode` and a next-gen
`ScriptedDecisionNode`; each variant's page outcomes were wired to dump nodes
that report `nodeState` on their first pass and again after their own callback
round trip. Variants `a` and `b` differ only in the scripted child's
`action.goTo("a")` / `goTo("b")`, so each is the other's control.

| Question | Observed | Control |
| --- | --- | --- |
| Config: a child with more than one outcome that is not last | `PUT` of the page node is `400 Invalid config: Only the last node in a page can have more than one outcome` | — |
| Callback order | Page order, child by child: `[Username, Password, Script…]` gives `NameCallback, PasswordCallback, <script's>`; `[Username, Script, Password]` puts the script's callbacks between the two | The order variant moved only the script and its callbacks moved with it |
| Scripted child's callbacks | Merged into the page's single `callbacks` array **inline at the child's position**, in the order the script queued them | The `nocb` variant (script queues none) returns only the two collector callbacks |
| `IDTokenN` input names | 1-based **position in the whole array, output-only callbacks included**: with a `TextOutputCallback` at index 2, the next `NameCallback` is `IDToken4` | — |
| Page outcomes | The **last** child's outcomes. `listOutcomes` (below) reported `["a","b"]` for a page ending in a two-outcome script, `["outcome"]` for one ending in `PasswordCollectorNode` | — |
| Script child's `goTo` | Becomes the page's outcome when the script is last: `goTo("a")` reached the `a` dump, `goTo("b")` the `b` dump. A script that is **not** last called `goTo("true")` and the page still left by `outcome` (the last child's) | Variants `a`/`b` |
| An outcome with no connection | The page submission returns **HTTP 500 with an empty body**. The tree `PUT` accepted the unwired tree | `unwired` vs `order`: identical except `order` wires the page's real outcome (`outcome`) and returns 200 |
| What the script child sees on submit | It runs with `callbacks` non-empty and sees **only its own** callbacks: `callbacks.getNameCallbacks()` held just its own answer, not the Username collector's | `nocb`: a script that queued none sees `getNameCallbacks().size() == 0` |
| Children run in order on submit | A last-position script read `username` and `password` (length 15, as submitted). A middle-position script read `username` but `password` was `null` | Same script, position is the only change |
| A script child with no callbacks | Runs **twice**: once when the page is first reached, invisibly (nothing in the first response), and once on submission; both runs see empty `callbacks`, and the submission run's `goTo` is the page outcome (`nocb` reached the `b` dump). The earlier "runs once" reading came from a `putShared` counter and was wrong — see the discovery rows below | Discovery rows below |
| Discovery run, counted outside AM (2026-11-02, `experiment-journey-page-discovery.sh`) | Each execution of the child `openidm.create`s a `managed/alpha_user` with a fresh UUID; records counted by IDM query. `[Username, script]`: **1** after the first POST, **2** after submit. `[script, Username]`: **1** / **2**. Both discovery records say `callbacks` empty | Baseline count 0 before every variant; positive control below |
| Discovery run of a callback-queuing child (positive control, `experiment-journey-page-discovery.sh`) | **1** record after the first POST (its `NameCallback` is on the page), **2** after submit: it runs again on submission with `callbacks` non-empty | Proves the channel sees a discovery-time run |
| State written in the discovery run (`experiment-journey-page-discovery.sh`) | **Kept**, not discarded: the submission run read the discovery run's shared counter (`1`) and wrote `2`, and the next node read `2` | Same counter in all three create variants |
| A `goTo` in the discovery run (`experiment-journey-page-discovery.sh`) | **Ignored**: a last-position callback-free child calling `goTo("b")` on its first execution still got the page shown; on submit it saw the flag its discovery run had written and took `a`, which the `a` dump confirmed | The flag is only written by the discovery run, so `a` proves both that it ran and that its `goTo` was not taken |
| A throw in the discovery run (`experiment-journey-page-discovery.sh`) | The first POST is **`401 Login failure`**; no page is returned and no record is created | The same page with a non-throwing callback-free child returns 200 with the page |
| State written by `UsernameCollectorNode` | `username`, **shared**: still readable after the next node's callback round trip | — |
| State written by `PasswordCollectorNode` | `password`, **transient**: readable by the next node (length 15), `null` and gone from `keys()` after that node's own callback round trip | `username` in the same snapshot survived |
| Other keys after a page | `nodeState.keys()` also listed **`pageNodeCallbacks`** (and `authLevel`, `realm`); `pageNodeCallbacks` survived a round trip, so it is shared | — |

The next-gen `nodeState` in these scripts had `get`, `getObject`, `putShared`,
`putTransient`, `mergeShared`, `mergeTransient`, `isDefined`, `remove` and
`keys` as functions; **`getShared` and `getTransient` are `undefined`**. So
shared versus transient can only be told apart by whether a key survives a
callback round trip, which is how every "shared"/"transient" cell in this file
was decided.

## Inner Tree

Measured 2026-11-02 with `scripts/experiment-journey-inner-tree.sh`. Outer
tree: a script writing `zzOuterShared` (shared) and `zzOuterTransient`
(transient) → `InnerTreeEvaluatorNode` → dump nodes T / F. Inner tree
(`innerTreeOnly: true`): a script that records what it can read and writes
`zzInnerShared` / `zzInnerTransient`, then a second script that reads the
transient key back and goes to the inner success node (`ok`) or the inner
failure node (`fail`).

| Question | Observed | Control |
| --- | --- | --- |
| Outcomes (`listOutcomes`) | `["true","false"]`; with `displayErrorOutcome: true`, `["true","false","error"]` | The flag is the only change |
| Outcome mapping | Inner tree reaching its **success** node → evaluator `true`; reaching its **failure** node → `false` (the outer journey continued down `false`, it did not fail) | `ok` vs `fail` differ only in the inner `goTo` |
| Outer **shared** state, inside | Visible (`zzInnerSawShared = "os"`) | — |
| Outer **transient** state, inside | **Not visible** (`null`) | Inside the inner tree, transient written by its first node **was** visible to its second (`zzInner2SawTransient = "it"`), so the binding reads transient there |
| Inner **shared** state, after | Visible (`zzInnerShared = "is"`), and survived the dump's round trip | — |
| Inner **transient** state, after | **Not visible** (`null`) | — |
| Outer transient, after the inner tree | **Visible again** (`zzOuterTransient = "ot"` on the dump's first pass, gone after its round trip) — the inner tree runs with its own transient state and the outer one is restored | — |
| `journey.name()` | The tree **currently executing**: the inner tree's name inside, the outer's outside | — |
| `journey.innerJourney()` | `true` inside, `false` in the outer tree | — |
| `journey.mustRun()` / `identityResource()` | `false` / `managed/alpha_user` in both | — |
| An `innerTreeOnly: true` tree invoked directly by `authIndexValue` | `400 {"code":400,"reason":"Bad Request","message":"No Configuration found"}` | The outer trees (`innerTreeOnly: false`) in the same run started normally |

## Identify Existing User

Measured 2026-11-02 with `scripts/experiment-journey-identity-nodes.sh`
(variants `ieu-*`). A seed script wrote exactly the keys under test, then the
node, then one dump node per outcome. One throwaway `alpha_user` existed with
`userName` U and `mail` M. Node config `identityAttribute: "mail"`,
`identifier: "userName"` unless stated.

| Seed (state before the node) | Outcome | State after (dump) |
| --- | --- | --- |
| shared `objectAttributes = {mail: M}` | `true` | `username = U`, `_id = <user _id>`, `objectAttributes = {mail: M, userName: U}` — all **shared** (survived a round trip) |
| shared `username = M`, no `objectAttributes` | `true` | `username = U` (overwritten with the `identifier` attribute's value), `_id`, shared `objectAttributes = {userName: U}` (created) |
| shared `username = U` (the userName, not the mail) | `false` | nothing written: no `_id`, no `objectAttributes` |
| the same, with `identityAttribute: "userName"` | `true` | as the second row — the control that the previous row failed because the lookup is on `identityAttribute` |
| transient `objectAttributes = {mail: M}` | `true` | as the first row; the node's `objectAttributes = {userName: U}` went to **shared** (visible after the round trip, when the transient copy was gone) |
| shared `objectAttributes.mail` and `username` both a non-existent address | `false` | nothing written |

So: the lookup value is `objectAttributes[identityAttribute]` (shared or
transient), or `username` when `objectAttributes` lacks it; it is matched
against `identityAttribute`; on a match it writes shared `_id`, shared
`username` = the user's `identifier` attribute, and adds
`objectAttributes[identifier]` in shared.

### Precedence when the two sources disagree (2026-11-04)

Measured with `scripts/experiment-journey-precedence.sh` (variants named in the
first column), run twice with the same result. Two throwaway users, A (`userName`
UA, `mail` MA) and B (`userName` UB, `mail` MB); same node config. A next-gen
seed script `putShared` exactly the keys shown; N is a `mail` nobody has.

| Variant: shared `username` / `objectAttributes.mail` | Outcome | State after (dump) |
| --- | --- | --- |
| `agree`: UA / MA (control) | `true` | `username = UA`, `_id` = A's, `objectAttributes = {userName: UA, mail: MA}` |
| `oaB-unameA`: UA / MB | `true` | **B**: `username = UB`, `_id` = B's — the seeded `username` is overwritten |
| `oaB-umailA`: MA / MB | `true` | **B** — a `username` that would itself find A (second row of the table above) still loses |
| `oaB-unobody`: N / MB | `true` | B — a non-matching `username` does not stop the lookup |
| `oanobody-unameA`: UA / N | `false` | no `_id`; `username` left as UA |
| `oanobody-umailA`: MA / N | **`false`** | no `_id`; `username` left as MA — **no fallback** to `username`, although `username = MA` alone is `true` |
| `page-agree`: Page[`UsernameCollectorNode`, `AttributeCollectorNode(mail)`] answered UA, MA | `true` | A, as `agree` |
| `page-disagree`: the same page answered UA, MB | `true` | B, as `oaB-unameA` |

So when `objectAttributes` holds `identityAttribute`, it is the **only** lookup
value: `username` is consulted only when that key is absent, a miss on
`objectAttributes` is `false` rather than a retry with `username`, and on a hit
the node overwrites `username` with the found user's `identifier`. The page
rows show the realistic shape (collector writes shared `username`, attribute
collector writes shared `objectAttributes.mail`) behaves exactly as the seeded
one.

### A blank or `null` lookup value, and letter case (2026-11-04)

Measured with `scripts/experiment-journey-defaults-case.sh`, run twice with the same result. One user A
stored with all-lower-case `userName` UA and `mail` MA. A callback-free probe
node before Identify recorded what `nodeState.get("objectAttributes")` held.
Same node config unless stated. `username = MA` was chosen so that a fallback
to `username` would find A (`true`) while a lookup of `""` would not.

| Variant | `objectAttributes.mail` before | Outcome | State after |
| --- | --- | --- | --- |
| `blank`: Page[`UsernameCollectorNode`, `AttributeCollectorNode(mail, required: false)`] answered MA, `""` | key **present**, `""` (a string) | **`false`** | no `_id`; `username` left as MA — `""` is looked up, no fallback |
| `filled`: the same page answered MA, MA (control) | MA | `true` | A |
| `seed-empty`: `putShared` `username = MA`, `objectAttributes = {mail: ""}` | `""` | `false` | as `blank` — so the collector's write, not the page, decides it |
| `seed-null`: the same with `{mail: null}` | key present, `null` | **`true`** | A; `objectAttributes.mail` stays `null` beside the added `userName` — a `null` counts as absent and `username` is used |
| `seed-nomail`: `objectAttributes = {sn}` (control) | key absent | `true` | A, as the 2026-11-02 second row |
| `ieu-mail-mixed`: `objectAttributes = {mail: <alternating-case MA>}` | mixed case | **`true`** | A; `username` = UA (stored case), `objectAttributes.mail` keeps the mixed-case input |
| `ieu-mail-exact` (control) | MA | `true` | A |
| `ieu-uname-mixed`: `username = <alternating-case UA>`, `identityAttribute: "userName"` | — | **`true`** | A; `username` overwritten with stored-case UA |
| `ieu-uname-exact` (control) | — | `true` | A |

So the precedence rule above reads "`objectAttributes[identityAttribute]`
unless it is absent **or `null`**": an empty string is a real lookup value and
fails. And the lookup is case-insensitive on both `mail` and `userName`, like
the underlying IDM `eq` (`10-managed-objects.md`); a node downstream that
compares `username` with what the user typed must expect the stored case.
Leading and trailing spaces in `objectAttributes.mail` are ignored the same way
(`true`), while a space inside the value is a miss (`false`) — verified
2026-11-04, `scripts/experiment-journey-escaping-query.sh`.

`listOutcomes`: `["true","false"]`.

## Create Object and Patch Object

Measured 2026-11-02 with `scripts/experiment-journey-identity-nodes.sh`
(variants `co-*`, `po-*`). Create config `identityResource:
"managed/alpha_user"`; Patch config the same plus `identityAttribute:
"userName"`, `patchAsObject: false`, `ignoredFields: []`. Results were read
back from IDM, not inferred from the outcome.

### Create Object (`listOutcomes`: `["CREATED","FAILURE"]`)

| Seed | Outcome | IDM | State after |
| --- | --- | --- | --- |
| shared `objectAttributes` with `userName`, `givenName`, `sn`, `mail` | `CREATED` | user exists | **no `_id`, no `username`** written — `keys()` was the seed's keys plus `authLevel`, `realm` |
| the same in **transient** `objectAttributes` | `CREATED` | user exists | nothing written; `objectAttributes` gone after a round trip (it was never copied to shared) |
| shared `objectAttributes = {userName}` only (required fields missing) | `FAILURE` | no user | nothing written |

So Create Object reads `objectAttributes` through the same transient-first
lookup as `nodeState.get`, and **does not return the new `_id` in state**. A
journey that needs the new user's id must look it up afterwards (for example
with Identify Existing User).

### Patch Object (`listOutcomes`: `["PATCHED","FAILURE"]`)

| Seed | Outcome | IDM `sn` after |
| --- | --- | --- |
| shared `objectAttributes = {userName: U, sn: "PatchedShared"}` | `PATCHED` | `PatchedShared` |
| transient `objectAttributes = {userName: U, sn: "PatchedTransient"}` | `PATCHED` | `PatchedTransient` |
| shared `username = U`, `objectAttributes = {sn: "PatchedByUsername"}` | `PATCHED` | `PatchedByUsername` — the object is found from `username` when `objectAttributes` lacks `identityAttribute` |
| shared `_id = <user _id>`, `objectAttributes = {sn: "PatchedById"}` | `FAILURE` | unchanged — **`_id` is not used** to find the object |
| shared `objectAttributes = {userName: "zzemu-nobody", sn}` (no such user) | **none — the journey ended `HTTP 401 {"code":401,"reason":"Unauthorized","message":"Login failure"}`** | unchanged |

The last two rows are worth separating in an emulator: no identifier at all is
the `FAILURE` outcome, but an identifier naming nobody aborts the whole journey
with the generic 401. Patch Object wrote nothing new into state.

## Attribute Collector

Measured 2026-11-02 with `scripts/experiment-journey-collectors.sh`. Variant
`ac`: a lone `AttributeCollectorNode` collecting `givenName`, `sn`, `mail`
(`validateInputs: false`, `required: false`). Variant `page`: a Page of
`ValidatedUsernameNode` (`validateInput: true`), the same collector with
`validateInputs: true, required: true`, and `ValidatedPasswordNode`
(`validateInput: false`), submitted first with `mail` empty and then complete.

| Question | Observed | Control |
| --- | --- | --- |
| Outcomes | `listOutcomes`: `["outcome"]` (also `ValidatedUsernameNode`, `ValidatedPasswordNode`) | — |
| Callbacks | One `StringAttributeInputCallback` per attribute, in `attributesToCollect` order. Outputs: `name` (the attribute), `prompt` (the schema title: `First Name`, `Last Name`, `Email Address`), `required`, `policies`, `failedPolicies`, `validateOnly: false`, `value: ""`. Inputs: `IDTokenN` **and `IDTokenNvalidateOnly`** | — |
| `policies` output | `{}` with `validateInputs: false`; with `true`, a JSON object holding the attribute's IDM policy (`policyRequirements: ["REQUIRED","VALID_TYPE", …]`, `policies`, `fallbackPolicies: null`, `name`) | The flag is the only change between the variants |
| `required` output | Mirrors the node's `required` flag | Same |
| State written | **Shared** `objectAttributes = {givenName, sn, mail}`, nothing at top level (`nodeState.get("mail")` is `null`) | Present after the dump's round trip |
| A non-required attribute left blank (2026-11-04, `scripts/experiment-journey-defaults-case.sh`) | The page is **not** re-sent; `objectAttributes.mail` is written as the empty string `""` — key present, `typeof` `string`, not `null`, not absent | The same page with `mail` filled wrote the value |
| A required attribute left empty | The whole page is **sent again**, with every submitted value echoed in its callback's `value` output — and `failedPolicies` on `mail` still `[]`; no message says why | The next submission, differing only in a filled `mail`, left the page |
| `ValidatedUsernameNode` | Emits `ValidatedCreateUsernameCallback` (outputs `policies`, `failedPolicies`, `validateOnly`, `prompt: "Username"`; no `name`). An empty submission came back with the page re-sent and `failedPolicies` naming `REQUIRED` and `MIN_LENGTH` — an array of **JSON-encoded strings** (`["{ \"policyRequirement\": \"REQUIRED\" }", …]`), not of objects. Writes **shared** `username` and shared `objectAttributes.userName` | — |
| `ValidatedPasswordNode` with `validateInput: false` | Emits a plain **`PasswordCallback`** (`prompt: "Password"`), not `ValidatedCreatePasswordCallback`. Writes **transient** `password` and a **transient** `objectAttributes = {password}` | Both gone after the dump's round trip, while `username` survived |

**`objectAttributes` lives in both channels at once, and `nodeState.get` does
not merge them.** After that page, `nodeState.get("objectAttributes")` returned
the transient map alone — `{password}` — and only after the round trip dropped
it did the same call return the shared `{mail, givenName, sn, userName}`. A
node that reads `objectAttributes` sees one map or the other, never their
union. (Create Object and Patch Object above read transient-only and
shared-only maps; a split map was not fed to them.)

**Not measured:** `ValidatedPasswordNode` with `validateInput: true`; whether
`IDTokenNvalidateOnly: true` suppresses the write.

## Transient state across a callback-free boundary

Measured 2026-11-02 with `scripts/experiment-journey-flow.sh` (part 1).
`S1` wrote `zzT` (transient) and `zzS` (shared) and went straight to `S2`,
which read both, then to a dump node.

| Boundary | `zzT` (transient) | Control |
| --- | --- | --- |
| `S1` → `S2`, no callback | **Visible** (`"t1"`) | — |
| `S1` → `S2` → dump, still no callback | Visible on the dump's first pass | — |
| The dump's own callback round trip | **Gone** (`null`, absent from `keys()`) | `zzS` (shared) survived the same round trip |

Transient state lives for the whole server-side run between two client
round trips, across any number of nodes, and is dropped at the round trip — the
same rule as `09-journeys.md` "What survives a callback round trip". Inner
trees are the exception (they get their own transient state, see above).

## Failure shapes

Measured 2026-11-02 with `scripts/experiment-journey-flow.sh` (part 2). Each
variant is a one-script tree, `true` → Success, `false` → Failure. `f-plain`
is the control the message variants are compared against.

| Variant | HTTP | Body |
| --- | --- | --- |
| `goTo("false")` → Failure node | 401 | `{"code":401,"reason":"Unauthorized","message":"Login failure"}` |
| `goTo("false").withErrorMessage("x")` | 401 | same, **`message: "x"`** |
| `withErrorMessage("x").goTo("false")` | 401 | same, `message: "x"` — order does not matter |
| `goTo("false").withLockoutMessage("x")` | 401 | `message: "Login failure"` — the lockout message did **not** appear |
| the script throws `new Error(…)` | 401 | `Login failure` (no hint of the exception) |
| `goTo("nope")`, an outcome the node does not declare | 401 | `Login failure` |
| `goTo("other")`, declared on the node but **not connected** in the tree | **500** | **empty body** (the page-node variant of this is in the Page node table) |
| `goTo("true")` → Success with shared `username` set | 200 | `{"tokenId":"…","successUrl":"/enduser/?realm=/alpha","realm":"/alpha"}` |
| `goTo("true")` → Success with **no `username`** | **500** | **empty body** |
| `goTo("true").withErrorMessage("x")`, no `username` | 500 | empty body — same as the row above |
| `goTo("true").withErrorMessage("x")` with `username` | 200 | a normal success; the message is ignored |
| `authIndexValue` naming no tree | 400 | `{"code":400,"reason":"Bad Request","message":"No Configuration found"}` — identical to invoking an `innerTreeOnly` tree |
| Resubmitting an **earlier** response (stale `authId`) after it was already answered once | **200** | Accepted: the node ran again from that point and gave the same result as the first submission |
| `authId` with its last 8 characters altered | 400 | `{"code":400,"reason":"Bad Request","message":"Authentication Error: Invalid AuthId Signature"}` |
| The callbacks resubmitted **without** `authId` | 200 | A **new** journey starts, and its first node receives the submitted callbacks as if resuming (the probe node took its `callbacks`-non-empty branch, with the first-pass-only key `maxAuthenticationSessionDuration` present) |

The `authId` is a signed, client-held token of roughly 2 KB (lengths 1,900–3,100
here, growing with state), and a replayed one is not rejected — replay
protection is not something an emulator should add.

The success-without-`username` row is the trap the `withErrorMessage` row
would otherwise hide: the 500 belongs to the missing principal, not to the
message.

## Declared outcomes

Measured 2026-11-02 (all scripts above call it): **`POST
/am/json/realms/root/realms/alpha/realm-config/authentication/authenticationtrees/nodes/{nodeType}?_action=listOutcomes`
with a node _config_ as the body** returns `[{"id": …, "displayName": …}]`.

| Node type | Body | Outcomes |
| --- | --- | --- |
| `UsernameCollectorNode`, `PasswordCollectorNode`, `AttributeCollectorNode`, `ValidatedUsernameNode`, `ValidatedPasswordNode` | `{}` or their config | `outcome` |
| `ScriptedDecisionNode` | `{}` | `[]` — its outcomes are the config's `outcomes` |
| `PageNode` | `{}` | `[]` |
| `PageNode` | `{"nodes": [<child refs>]}` | the **last** child's outcomes, read from that child's stored config by `_id` (`["a","b"]`, `["outcome"]`, `["true"]` measured) |
| `InnerTreeEvaluatorNode` | `{"displayErrorOutcome": false}` / `true` | `true,false` / `true,false,error` |
| `IdentifyExistingUserNode` | `{}` | `true,false` |
| `CreateObjectNode` | `{}` | `CREATED,FAILURE` |
| `PatchObjectNode` | `{}` | `PATCHED,FAILURE` |

The per-instance form, `POST …/nodes/{nodeType}/{id}?_action=listOutcomes`,
is **`501 listOutcomes not supported`** for every type tried (Page, Scripted
Decision, Username Collector).

## Callbacks versus `goTo`

Measured 2026-11-02 with `scripts/experiment-journey-flow.sh` (part 3),
re-running the measurement `99-quirks-and-open-questions.md` (2026-09-30)
recorded as supplied but never re-run.

| Variant | Step 1 | Step 2 |
| --- | --- | --- |
| `cb-and-goto`: on its first pass queues a `TextOutputCallback` **and** calls `goTo("true")` | **The callback is returned**; the outcome is discarded | Resubmitting runs the script **again** (its pass counter, kept in shared state, read `2`; `callbacks.isEmpty()` was `false`), its `goTo("true")` is now acted on and the dump node runs |
| `goto-only`: identical minus the callback (control) | The dump node runs straight away; pass counter `1` | — |

Confirmed: queued callbacks win over `goTo` on the same pass, the node is
re-executed from the top on resume, and shared state written on the
suspended pass survives into the resumed one.

## Tree deletion removes the tree's nodes

Measured 2026-11-02 with `scripts/experiment-journey-flow.sh` (part 4):
a `ScriptedDecisionNode` referenced by a tree and one referenced by none both
`GET` 200; after `DELETE` of the tree (200) the first `GET`s **404** and the
second still 200. The page experiments showed the same for nodes that are only
page children. Scripts are **not** deleted with the tree (each `DELETE` of a
script afterwards was 200).

## Values a journey script hands to IDM and `httpClient`

Measured 2026-11-04 with `scripts/experiment-journey-precedence.sh`; the rows
are in `12-script-bindings-matrix.md` under "`httpClient.send` body
serialization". In short: `openidm.create` content with `description:
undefined` succeeds and stores nothing (as an omitted key or `null` does, never
the string `"undefined"`); `httpClient.send` with top-level `body: undefined`
sends the 4-byte body `null`, as `body: null` does, while omitting `body` sends
an empty one; `{count: 1}` goes out as `{"count":1.0}`. On a property with a schema `default`
the difference shows: `undefined` is refused `NOT_NULL` exactly as `null` is,
so the default never lands (2026-11-04, `scripts/experiment-journey-defaults-case.sh`;
same section of doc 12).

## What a script can put in the audit log (2026-11-25)

Measured 2026-11-25 on the sandbox tenant, realm `alpha`, with one-script
throwaway trees (`script` node → a sink node that emits a hidden callback so the
response proves which outcome was taken). Each request carried its own
`x-forgerock-transactionid`, and the events were read back with
`aic logs tx <id> --source am-authentication,am-access,am-core`. The probe was
an ad hoc driver over `scripts/experiment_journey_lib.py`, not committed.

**`auditEntryDetail` is an output variable the script assigns, not a binding it
is handed.** Before assignment it does not exist (`typeof` is `"undefined"`,
reading it is a `ReferenceError`). After the script assigns it, the node reads
it back and AM writes it into that node's `AM-NODE-LOGIN-COMPLETED` record as
`entries[0].info.nodeExtraLogging.auditInfo`. It works on next-gen (2.0) and
legacy (1.0) decision nodes alike.

| Script (next-gen ends `action.goTo("true");`, legacy ends `outcome = "true";`) | Observed in `entries[0].info` | Engines |
| --- | --- | --- |
| `auditEntryDetail = {"k": "v", "list": ["a", "b"]};` (implicit global) | `nodeExtraLogging: {"auditInfo": {"k": "v", "list": ["a", "b"]}}` | both |
| `var auditEntryDetail = {…};` at top level | the same | both |
| `auditEntryDetail = "KDEMO-STR";` | `nodeExtraLogging: {"auditInfo": "KDEMO-STR"}` | both |
| `auditEntryDetail = ["KDEMO-A", "KDEMO-B"];` (top-level **array**) | **Rejected**: client gets `401 Login failure`; `am-core` WARN `script auditEntryDetail not type String or Map`; no node record; `AM-TREE-LOGIN-COMPLETED` `FAILED` with `nodeExtraLogging: {exception: "An exception occurred during the authentication process", failureReason: "Invalid auditEntryDetail type from script"}`. An array is fine **inside** an object (first row) | both |
| `{"n": 1, "f": 1.5, "b": true, "z": null, "o": {"x": "y"}, "e": "", "u": undefined}` | `auditInfo: {"b": true, "e": "", "f": 1.5, "n": 1, "o": {"x": "y"}, "u": null, "z": null}`: numbers, booleans, nested objects and `""` survive; `undefined` becomes `null`; the keys came back in alphabetical order | next-gen |
| `auditEntryDetail = {};` | `nodeExtraLogging: {"auditInfo": {}}` | next-gen |
| `auditEntryDetail = null;` | no `nodeExtraLogging` key at all (same as never assigning) | next-gen |
| Assigned, then `goTo("false")` to a sink node | `AM-NODE-LOGIN-COMPLETED` with `nodeOutcome: "false"` **and** `nodeExtraLogging.auditInfo` | both |
| Assigned, then `goTo("false")` to the Failure node | that node's `AM-NODE-LOGIN-COMPLETED` carries `auditInfo` (`nodeOutcome: "false"`); the following `AM-TREE-LOGIN-COMPLETED` (`FAILED`) has `nodeExtraLogging: {failureReason: "Login failure"}` and no `auditInfo` | next-gen |
| Assigned, then the script throws (`throw new Error("KDEMO-THROWN")`; legacy throws a string) | **Not emitted.** No `AM-NODE-LOGIN-COMPLETED` for the node; the `AM-TREE-LOGIN-COMPLETED` `nodeExtraLogging` is only the usual `{exception, failureReason}` and the assigned value appears nowhere in `am-authentication` | both |

So the value is a **string or a map (JSON object)**, shown under the fixed key
`auditInfo`; an object holding an array of entries is the way to carry several.
It is per node execution and rides the completion record, so it exists only for
nodes that finish. The legacy runs also logged the unrelated `am-core` WARN
`Found an action result from scripted node, but it was not an Action object`
(legacy `outcome = "…"` still took the right path).

Other probes, all from a script that did **not** assign `auditEntryDetail`:

| Question | Observed |
| --- | --- |
| `action.goTo("true").withDescription(…).withErrorMessage(…).withStage(…).withHeader(…).putSessionProperty(…)` | `AM-NODE-LOGIN-COMPLETED` identical to a bare `goTo("true")`; none of the strings reaches `am-authentication` |
| `nodeState.putShared("auditEntryDetail", {…})`, `putTransient("nodeExtraLogging", "…")` | no effect on the record. Only an **assignment to the script variable** works |
| The `action` builder | Methods: `goTo, withDescription, withErrorMessage, withLockoutMessage, withStage, withHeader, withMaxIdleTime, withMaxSessionTime, withIdentifiedUser, withIdentifiedAgent, putSessionProperty, removeSessionProperty, suspend`; none is an audit method |
| Global scope before assignment | next-gen: `samlApplication, logger, callbacks, idRepository, jwtAssertion, utils, context, action, callbacksBuilder, openidm, requestCookies, cookieName, policy, httpClient, journey, requestParameters, require, cacheManager, secrets, oauthApplication, emailService, locales, systemEnv, requestHeaders, nodeState, resumedFromSuspend, scriptName, realm, jwtValidator`; legacy: `httpClient, sharedState, logger, requestParameters, callbacks, secrets, idRepository, systemEnv, requestHeaders, nodeState, resumedFromSuspend, context, scriptName, realm, transientState`. No audit name in either |
| A custom outcome name, `goTo("KDEMO-OUTCOME-A")` on a node declaring two such outcomes | `info.nodeOutcome` is `"KDEMO-OUTCOME-A"` |
| A script that throws (no assignment) | Client `401 Login failure`; no node record; one `AM-TREE-LOGIN-COMPLETED` `FAILED` with `nodeExtraLogging: {exception: "An exception occurred during the authentication process", failureReason: "javax.script.ScriptException: java.util.concurrent.ExecutionException: javax.script.ScriptException: <String(thrown)> (<script name>#1) in <script name> at line number 1 at column number 0"}`. `logger` calls made before the throw are still in `am-core` |

The earlier probe that only read `typeof auditEntryDetail` (and listed the
global scope) found it undefined; that was correct but did not test assignment.

Where the script `logger` lands is in `08-logs.md`, "Script logger lines".

**A succeeding tree** (measured 2026-11-25, two runs: a journey that identified
a user by `_id`, and one through an Anonymous User node; both reached Success
with `200`). Its `AM-TREE-LOGIN-COMPLETED` has `result: "SUCCESSFUL"`,
`entries[0].info` = `{authLevel, ipAddress, treeName}` and no
`nodeExtraLogging`, `principal: ["<username>"]` and
`userId: "id=<universal id>,ou=user,o=alpha,ou=services,ou=am-config"`, where
the universal id is the identified user's `_id`, or the anonymous user's name.
A journey that reaches Success with no identified user returns `500` with an
empty body and was not read back.

**Not measured.**
Assignment inside a Page node child, or in a node that suspends or sends
callbacks. A size limit on the value. Whether any other script context (OAuth2,
SAML, policy) accepts the assignment. Whether this key is emitted at the
production log level: the sandbox is verbose (`08-logs.md`), and the
audit topic's own filtering is unmeasured.

## Built-in node types (2026-11-25)

Measured on the sandbox tenant, realm `alpha`, 2026-11-25, next-gen scripted
decisions. Each node sat in a throwaway tree (`kdemo-nt-*` trees, scripts and
users, all deleted afterwards) behind a seed script that put the state under
test, and a dump script reported `nodeState.keys()` and values, on its first
pass and again after its own callback round trip (a key that survives it is
shared). The drivers were one-off scripts built on `experiment_journey_lib.py`
and are not committed; re-measure before building on a row you doubt.
`listOutcomes` is quoted from the tenant.

### Data Store Decision (`listOutcomes`: `["true","false"]`)

Config `{}`. Driven with state put by a script (a page of collectors gave the
same answers).

| Question | Observed | Control |
| --- | --- | --- |
| Right password | `true`; **no state written** (no `_id`, `universalId` or `authLevel` change) | Keys before and after identical |
| Wrong password, another user's password, empty password, unknown user | `false` | Same tree, right password: `true` |
| Letter case of the username | Case-insensitive: `ALICE`-style input of an existing `userName` is `true` | Exact case: `true` |
| A mail address as the username | `false`: the lookup is `userName` only | The same user's `userName`: `true` |
| Where the username and password are read | Username from **shared** state only: a username put in transient state **aborts the journey with 401 `Login failure`**, as does no username or no password. The password is found in either bucket | Shared username plus transient or shared password: `true` |

### Set Session Properties (`listOutcomes`: `["outcome"]`)

| Question | Observed | Control |
| --- | --- | --- |
| Config validation (node `PUT`) | `400 Invalid config: At least one of the following properties must be set: properties, maxSessionTime, maxIdleTime` for `{}` and for `{"properties":{}}`. `400 Invalid config: Provided properties cannot include system session properties: [clientType, FullLoginURL, Principals, XUSRef, Locale, amlbcookie, UserId, loginURL, logoutHooks, Service, HostName, UserToken, IndexType, webhooks, UserProfile, AuthLevel, CharSet, authInstant, Host, AMCtxId, OidcSid, sun.am.UniversalIdentifier, Organization, Principal]` for any of them, also beside an allowed one | An allowed name (`kdemoNtProp`) and `maxIdleTime` alone: accepted |
| Effect | A second journey given the first's session cookie saw every configured property in `existingSession` beside the system ones. `maxIdleTime: 7` / `maxSessionTime: 11` (minutes) moved `getSessionInfo`'s `maxIdleExpirationTime` / `maxSessionExpirationTime` to 7 and 11 minutes after login | A node with neither: 45 and 120 minutes (the realm defaults) |
| `getSessionProperties` and `getSessionInfo` | Return only the allow-listed properties (`AMCtxId`): a configured property is **not** visible there, only in `existingSession` | — |

### Set Persistent Cookie (`listOutcomes`: `["outcome"]`)

| Question | Observed | Control |
| --- | --- | --- |
| Signing key | With neither `hmacSigningKey` nor a secret label configured the journey ends **500 with an empty body**. With a base64 256-bit `hmacSigningKey`: success | Same tree with the key |
| What the response sets | Three `Set-Cookie`: the session cookie, the persistent cookie, then `amlbcookie`. The persistent one is named `persistentCookieName`, holds a ~1.5 KB value, and has `Expires` = login + `maxLife` **hours** (9 gave +9 h, 3 gave +3 h), `Path=/`, the tenant `Domain`, `Secure`, `HttpOnly`, `SameSite` | `useHttpOnlyCookie: false` dropped `HttpOnly` only |
| `useSecureCookie: false` | `Secure` was **still set** (the platform forces it) | `true`: set |
| `sameSite` | `LAX`, `STRICT`, `NONE` came back as `lax`, `strict`, `none` | — |

Not measured: the value's claims (it looks like a signed JWT), the effect of
`idleTimeout`, and signing through a secret label.

### One Time Password Generator (`listOutcomes`: `["outcome"]`)

| Question | Observed | Control |
| --- | --- | --- |
| State written | **Shared** `oneTimePassword` (a string of digits, **leading zeros kept**, `00241136`) and `oneTimePasswordTimestamp` (a **number**, epoch **seconds**). Both survived the callback round trip | `maxAuthenticationSessionDuration` (transient) did not |
| Length | 6, 7 and 8 gave that many digits; a different password every run. A `length` of **9, 10, 11, 12 or 20 passes the node `PUT` but fails the journey with 401 `Login failure`** | A `length` of 0, 3, 4 or 5 is refused at `PUT`: `400 Data validation failed for the attribute, One Time Password Length`. No `length` is accepted, and ran as 8 |

### Retry Limit Decision (`listOutcomes`: `["Retry","Reject"]`)

| Question | Observed | Control |
| --- | --- | --- |
| Counter | **Shared** `<node id>.retryCount`, a number that survives round trips; the node id is the key's prefix, so two retry nodes count separately | — |
| Behaviour | `retryLimit: N` gives `Retry` for visits 1 to N (the count reads 1 to N), then `Reject` on every later visit with the count left at N. Limits 1, 2, 3 and an unset limit (default 3) all fit that | `retryLimit: 0` is refused at `PUT`: `400 Data validation failed for the attribute, Retry Limit. Reason: The value must be greater than zero` |
| `incrementUserAttributeOnFailure: true` | With `_id` and `username` in shared state, a limit of 2 reached `Reject` and the user's managed record held **no changed attribute** (`GET` with `_fields=*`) | Not measured: AM's own invalid-attempts attribute, which IDM may not expose |

### Accept Terms and Conditions (`listOutcomes`: `["outcome"]`)

| Question | Observed | Control |
| --- | --- | --- |
| Callback | One `TermsAndConditionsCallback`, outputs `version`, `terms`, `createDate` (the realm's Terms and Conditions service); `IDToken1` starts as the boolean `false` | — |
| Answers | `true` or the string `"true"` takes `outcome`. `false`, `"false"` and `""` send the same callback again, writing nothing | — |
| State | **Shared** `termsAccepted`, a **string** `{ "acceptDate": "<ISO instant, nanoseconds>", "termsVersion": "<version>" }`, and `termsVersion` | Both survived the round trip |

### CAPTCHA (`product-CaptchaNode`, `listOutcomes`: `["true","false"]`)

Only the callback was measured: a `ReCaptchaCallback` with outputs
`recaptchaSiteKey`, `captchaApiUri`, `captchaDivClass`, `reCaptchaV3`,
`disableSubmission` (from `siteKey`, `apiUri`, `divClass`, `reCaptchaV3`,
`disableSubmission`) and an empty-string `IDToken1`. A fake token, and an empty
one, ended the journey with 401 `Login failure` (not `false`), with the
provider's published always-pass test keys as well as with fake ones, so
neither outcome was reached; verification calls a provider that the harness
cannot reproduce, and the emulator takes the verdict from the caller.

## Not yet measured

None of these was exercised; do not build on them as measured:

- Create Object / Patch Object fed a **split** `objectAttributes` (part
  shared, part transient, as a Validated Password page leaves it); Patch
  Object with `patchAsObject: true`, or with `ignoredFields`.
- `ValidatedPasswordNode` with `validateInput: true` (expected to emit
  `ValidatedCreatePasswordCallback`; not observed), and the effect of
  `IDTokenNvalidateOnly: true` on any validated callback.
- A Page node holding a node that is not a collector or scripted decision; a
  scripted child that calls `action.suspend()` or `withHeader` inside a page.
- `withLockoutMessage` on an account that is actually locked out.
- Inner Tree with `displayErrorOutcome: true` when the inner tree throws; an
  inner tree that sends callbacks; nested inner trees.
- Replaying the `authId` of a step that led to **success** (whether a second
  session is minted).
- Message Node, Choice Collector, Polling Wait, Email Suspend and other node
  types not named above or in "Built-in node types".
- Any of the nodes in "Built-in node types" inside a Page, inside an inner
  tree, or with a configuration other than the one listed there.
- Any of this in realm `bravo` or under a `noSession` / `transactionalOnly`
  tree.
