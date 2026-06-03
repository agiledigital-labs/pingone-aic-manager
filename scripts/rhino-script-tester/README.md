# Rhino Script Tester

Temporary harness for probing AM scripted decision runtime behavior in the sandbox tenant.

The goal is to keep the test loop small:

1. Set up one next-gen scripted decision node and one journey.
2. Edit a standalone JavaScript test script.
3. Upload only the script body.
4. Invoke the existing journey.
5. Fetch transaction logs only when the invoke response is not enough.

## Prerequisites

- `cargo build --locked --offline` has built `target/debug/aic`.
- The local `aic` agent is running, unlocked, and has a token for the sandbox tenant.
- `curl`, `jq`, and `base64` are available.
- For logs, `.envrc` exports either:
  - `LOG_API_KEY_ID` and `LOG_API_KEY_SECRET`, or
  - `API_KEY_ID` and `API_KEY_SECRET`.

No log API keys should be committed. `.envrc` is ignored by the repo.

## Tenant Resources

By default the harness uses these sandbox resources:

- Script: `AIC Rhino Let Probe`
- Journey: `AIC-Rhino-Let-Probe`
- Script ID: `2e87a29c-0e30-4d85-bf0e-a1c0a11e7001`
- Node ID: `2e87a29c-0e30-4d85-bf0e-a1c0a11e7002`

The script is created as a next-gen scripted decision script:

- `context: AUTHENTICATION_TREE_DECISION_NODE`
- `evaluatorVersion: 2.0`

Override `BASE`, `REALM_PATH`, `TENANT`, `SCRIPT_NAME`, `TREE_NAME`, `SCRIPT_ID`, or `NODE_ID` if needed.

## One-Time Setup

Run this only when the tenant resources are missing or the journey shape changes:

```bash
scripts/rhino-script-tester/setup.sh
```

Setup uploads the default test script, creates or updates the scripted decision node, and creates or updates the journey. It does not invoke the journey.

## Normal Test Loop

Use `test-cycle.sh` for the usual edit/upload/run cycle:

```bash
scripts/rhino-script-tester/test-cycle.sh scripts/rhino-script-tester/scripts/rhino-let-behaviour.script.js
```

That script calls:

```bash
scripts/rhino-script-tester/update-script.sh <script.js>
scripts/rhino-script-tester/run-journey.sh
```

`update-script.sh` only updates the AM script body. It does not update the tree or node.

## Current Probe Scripts

- `scripts/rhino-let-behaviour.script.js` intentionally uses `let`.
- `scripts/rhino-var-control.script.js` is a `var`-only control script.

Current observed behavior in the sandbox:

- `rhino-var-control.script.js` returns HTTP 200 and the expected hidden callback JSON.
- `rhino-let-behaviour.script.js` fails before callbacks are returned. Logs report a Rhino parse error at the first `let` declaration: `missing ; before statement`.

This gives us a working baseline for validating ESLint rules against real Rhino behavior.

## Logs

When `run-journey.sh` fails, copy the printed transaction id and fetch logs:

```bash
scripts/rhino-script-tester/get-transaction-logs.sh <transaction-id>
```

By default logs are written to:

```text
tmp/rhino-script-tester/logs.json
```

The repo-local `tmp/` directory is ignored. Logs can be large, so summarize or filter them before sharing output.
