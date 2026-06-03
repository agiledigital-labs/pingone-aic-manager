// Legacy-only scripted-decision globals (evaluatorVersion 1.0). Layered on top
// of rhino + common + decision-node-base.
//
// These were removed/replaced in next-gen: `sharedState`/`transientState` give
// way to nodeState.putShared/putTransient (both verified ABSENT in next-gen,
// 2026-06-03), and `JavaImporter` (Java allowlist) is gone in next-gen.
//
// The legacy `logger` (classic Debug) shape comes from legacy-common.d.ts, which
// this leaf includes instead of the next-gen logger. NOTE: `nodeState.get()`
// returns a Java JsonValue on the legacy engine (needs .asString()/.asMap()),
// and `httpClient`'s legacy shape is unverified — those shape differences from
// decision-node-base.d.ts / common.d.ts remain documented imperfections.

interface MutableState {
  get: (key: StringLike) => any;
  put: (key: StringLike, value: any) => void;
}
declare const sharedState: MutableState;
declare const transientState: MutableState;

interface JavaClass {}
declare const JavaImporter: (...classes: JavaClass[]) => void;

// Legacy nodeState also exposes these (undocumented; verified 2026-06-04). Merged
// onto the shared NodeState (decision-node-base.d.ts) only on the legacy leaf.
interface NodeState {
  isDefined(key: StringLike): boolean;
  remove(key: StringLike): void;
}
