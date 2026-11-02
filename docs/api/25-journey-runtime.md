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

Sandbox tenant, realm `alpha`, **2026-11-02**. Every row names the
`scripts/experiment-journey-*.sh` run that established it. Rows not so marked
are not measured.

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
| A script child with no callbacks | Not shown on the first response; on submission it runs once with empty `callbacks` and its `goTo` is the page outcome (`nocb` reached the `b` dump) | — |
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

Not yet measured.

## Identify Existing User

Not yet measured.

## Create Object and Patch Object

Not yet measured.

## Attribute Collector

Not yet measured.

## Transient state across a callback-free boundary

Not yet measured.

## Failure shapes

Not yet measured.

## Declared outcomes

Not yet measured.

## Callbacks versus `goTo`

Not yet measured.

## Not yet measured

- Everything above, pending the experiments.
