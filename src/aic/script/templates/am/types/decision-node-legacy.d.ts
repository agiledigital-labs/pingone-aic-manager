// Legacy-only scripted-decision globals (evaluatorVersion 1.0). Layered on top
// of rhino + common + decision-node-base.
//
// These were removed/replaced in next-gen: `sharedState`/`transientState` give
// way to nodeState.putShared/putTransient (both verified ABSENT in next-gen,
// 2026-06-03), and `JavaImporter` (Java allowlist) is gone in next-gen.
//
// NOTE: the common.d.ts / decision-node-base.d.ts bindings this overlay sits on
// use next-generation shapes. Legacy shape differences (e.g. logger
// error/message/warning, JsonValue nodeState returns) are not yet probed — see
// matrix open item #1. Until then this overlay only adds the legacy-only names.

interface MutableState {
  get: (key: StringLike) => any;
  put: (key: StringLike, value: any) => void;
}
declare const sharedState: MutableState;
declare const transientState: MutableState;

interface JavaClass {}
declare const JavaImporter: (...classes: JavaClass[]) => void;
