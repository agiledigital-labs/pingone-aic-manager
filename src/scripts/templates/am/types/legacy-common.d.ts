// Classic AM `logger` shape, shared by the legacy scripted-decision engine and
// the other (mostly unmigrated) AM script contexts. Layered on rhino + common.
//
// Runtime-verified on the legacy engine (2026-06-04): `logger` is the classic
// Debug object — error/message/warning plus the *Enabled guards. The slf4j-style
// trace/debug/info/warn are ABSENT here (those are the next-gen shape; see
// nextgen-common.d.ts).
//
// The METHOD NAMES are the classic Debug set, but the argument handling behind
// them is slf4j's, not `Debug.message(String)`'s: extra arguments are accepted
// and bound to `{}` in the message, and a trailing throwable is logged as an
// `exception` field (verified 2026-08-27 at all three levels —
// `fixtures-legacy/legacy-logger-levels.script.js`). Declaring these
// single-argument was wrong in the direction that matters: it rejected the
// two-argument calls real scripts write, most visibly in the legacy
// access-token-modification context, whose bindings otherwise say nothing about
// logging. `LogFunction` in common.d.ts carries the `{}` arity check.
interface Logger {
  error: LogFunction;
  message: LogFunction;
  warning: LogFunction;
  errorEnabled(): boolean;
  messageEnabled(): boolean;
  warningEnabled(): boolean;
}
declare const logger: Logger;
