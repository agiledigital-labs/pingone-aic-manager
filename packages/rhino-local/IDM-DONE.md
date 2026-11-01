# Slice B working note

Implementation is in progress in `src/idm/`. REST and config types, request-local
provenance, CREST errors and the filter parser are in place. No public package
entry point yet. No Java changes.

Decisions: retain `{status, body}` and add optional `rawBody` for the measured
invalid JSON / empty endpoint responses. Config will be frozen and shared;
forks will deep-copy state. A named `_queryId` has no measured error in doc 24;
it will fail as missing rather than inventing a measured answer.

Baseline: host suite running; requested baseline is 1352 passed, 4 skipped.
Gates: none complete yet. Provenance inventory and final counts follow as the
implementation settles.
