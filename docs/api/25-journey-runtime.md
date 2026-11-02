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

Not yet measured.

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
