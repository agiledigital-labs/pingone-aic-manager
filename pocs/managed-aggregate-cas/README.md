# Managed-aggregate CAS POC

Probes for whether a revision-less managed `PATCH` plus a pure `onUpdate` can
hold derived state and an audit trail without a hand-written retry loop. The
measured answer and the pattern are in `RESULTS.md` and
`docs/managed-object-aggregates.md`.

Sandbox only. `lib.mjs` reads the current context and refuses anything except
`sandbox` (override with `POC_CONTEXT` only if you mean it).

## Environment

```bash
export AIC=<aic binary>
export AIC_PROJECT=<checkout whose agent is unlocked on the sandbox context>
export SENSITIVE_DENYLIST=$HOME/.config/aic/denylist.txt
cd pocs/managed-aggregate-cas
```

Every `$AIC` invocation needs `--no-prompt`. The scripts add it. Do not write
the tenant hostname, or any other tenant identifier, into a committed file.
`.state/` is gitignored and holds the tenant's managed schema; do not force-add
it.

## Install

```bash
node setup.mjs install
```

Backs up `config/managed` and `config/repo.ds` under `.state/`, creates
`alpha_poc_agg`, `alpha_poc_child` and `alpha_poc_sink`, declares one
`repo.ds`-mapped relationship (`children`) and one generic pair (`gchildren`),
and installs the hooks. `node setup.mjs hooks` pushes the hook sources again.
`NOTIFY=1 node setup.mjs notify` sets `notifySelf` on the parent and `notify` on
the child; `NOTIFY=2` swaps the ends; no `NOTIFY` clears both.

## Experiments

```bash
node experiments.mjs E1
node experiments.mjs E16 E15 E13 E18
E14_TRACE=0 E14_N=50,100,200 node experiments.mjs E14
E14_N=100 node experiments.mjs E14
node experiments.mjs E20
```

One id or several, in order. Raw JSON goes to `.state/results/`. `REPEATS`
defaults to 3. E15's self-patches are bounded in the hooks and time out at 90 s
per request. E18 stops at the first create that is not successful. E14 stops
climbing at the first non-200. E13 creates the `pocagg` endpoint. E20 sends ten
concurrent appends under one transaction-id stem and summarises `idm-activity`.

## Trace

```bash
node trace.mjs <txid-or-stem>
```

Fetches `idm-access` and `idm-activity` with `aic logs tx --wait --output` and
prints, per stored transaction id, the write count by operation and object type,
the object ids, and the access event's status and latency. Reads are not in
those sources. A hook counts a read by logging
`context.transactionId.transactionId.value` with the path it read. Matching is a
prefix match: zero-pad ids that share a stem (`<stem>-01`, not `<stem>-1`).

## Teardown

```bash
node setup.mjs teardown
```

Deletes the POC records, the mapped relationship and its `repo.ds` entries, the
three types, and the `pocagg` and `poctrace` endpoint configs. It then prints
whether `config/managed` equals `.state/managed-before.json` and whether
`repo.ds` `genericMapping` equals `.state/repo.ds-before.json`. Both lines must
say `true`. If either does not, stop. Do not hand-edit the tenant config. The
`repo.ds` backup taken before the POC is `.state/repo.ds-before.json`.

## Files

| File                    | What it is                                                                                                       |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `PLAN.md`               | The experiment list and the pattern the probes were aimed at.                                                    |
| `RESULTS.md`            | The measured tables.                                                                                             |
| `DONE.md`               | What this run actually did, and where it diverged.                                                               |
| `lib.mjs`               | HTTP helper. Mints a bearer with `aic whoami --token` and refuses a non-sandbox context.                         |
| `setup.mjs`             | Install, hook push, notification flags, teardown.                                                                |
| `experiments.mjs`       | E1–E16, E18–E20.                                                                                                 |
| `trace.mjs`             | Write summary for one transaction id or stem.                                                                    |
| `hooks/onUpdate.js`     | One hook, several modes (`fold`, `audit`, `children`, `selfpatch`, `plain`).                                     |
| `hooks/postUpdate.js`   | `sink`, `throw`, or a guarded self-patch.                                                                        |
| `endpoints/poctrace.js` | Source of the endpoint that did one read and one query for the single-request trace. E13 writes `pocagg` itself. |
