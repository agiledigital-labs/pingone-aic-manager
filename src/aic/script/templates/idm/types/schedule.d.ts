// IDM scheduled-task bindings. Layered on rhino + common.
//
// Schedule scripts run on a trigger (not an HTTP request), so the endpoint
// `request`/`context` shapes do NOT apply. Their actual binding set is not yet
// runtime-verified (see docs/api/12-script-bindings-matrix.md, IDM open items),
// so this overlay deliberately adds nothing beyond the common logger/openidm.
// Fill in once an IDM schedule probe exists; until then schedules type-check
// against the common bindings only.

// (intentionally empty — schedule-specific bindings unverified. Do NOT add a
// top-level import/export here: that would turn this into a module and stop the
// common.d.ts globals from resolving in schedule scripts.)
