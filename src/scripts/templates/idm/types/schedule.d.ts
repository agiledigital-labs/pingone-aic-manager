// IDM scheduled-task bindings. Layered on rhino + common.
//
// Working assumption (not yet runtime-probed — schedules are cron-triggered and
// harder to invoke synchronously): scheduled scripts have the SAME bindings as
// custom endpoints — logger, openidm, context (all from common.d.ts) — with two
// exceptions:
//   1. no `request` binding (there's no incoming CREST request);
//   2. the return value is not a CREST response (no resource/query-result shape;
//      a scheduled run's return is not surfaced like an endpoint's).
// So this overlay adds nothing: schedules type-check against the IDM common
// bindings. The IDM engine's syntax limits (no default params / const-in-for —
// verified on endpoints) apply here too via idm/eslint.config.js.

// (intentionally empty — see above. Do NOT add a top-level import/export here:
// that would turn this into a module and stop the common.d.ts globals from
// resolving in schedule scripts.)
