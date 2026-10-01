import { defineCase } from "../../src/case/index.ts";
import type {
  Case,
  CaseInit,
  Expect,
  Given,
  JsonObject,
  RecordedEffects,
  CompleteRecordedEffects,
  StateBucket,
} from "../../src/case/index.ts";

export function makeCase(
  overrides: {
    name?: string;
    script?: string;
    outcomes?: readonly string[];
    given?: Given;
    expect?: Expect;
  } = {}
): Case {
  const init: CaseInit = {
    name: overrides.name ?? "example",
    script: overrides.script ?? "am/decision-node/example.js",
    expect: overrides.expect ?? { outcome: "true" },
  };
  if (overrides.outcomes !== undefined) {
    init.outcomes = overrides.outcomes;
  }
  if (overrides.given !== undefined) {
    init.given = overrides.given;
  }
  return defineCase(init);
}

export function makeEffects(
  overrides: Partial<RecordedEffects> = {}
): CompleteRecordedEffects {
  const effects: CompleteRecordedEffects = {
    outcome: "true",
    sharedState: { initial: {}, final: {} },
    transientState: { initial: {}, final: {} },
    secureState: { initial: {}, final: {} },
    sessionProperties: { initial: {}, final: {} },
    callbacks: [],
    openidm: [],
    http: [],
    logs: [],
    identityWrites: [],
    evidence: {
      stateBuckets: "exact",
      ambientState: {},
      unbucketedState: [],
      unobservedChannels: [],
    },
  };
  if ("outcome" in overrides) {
    effects.outcome = overrides.outcome ?? null;
  }
  if (overrides.sharedState !== undefined) {
    effects.sharedState = overrides.sharedState;
  }
  if (overrides.transientState !== undefined) {
    effects.transientState = overrides.transientState;
  }
  if (overrides.secureState !== undefined) {
    effects.secureState = overrides.secureState;
  }
  if (overrides.sessionProperties !== undefined) {
    effects.sessionProperties = overrides.sessionProperties;
  }
  if (overrides.callbacks !== undefined) {
    effects.callbacks = overrides.callbacks;
  }
  if (overrides.openidm !== undefined) {
    effects.openidm = overrides.openidm;
  }
  if (overrides.http !== undefined) {
    effects.http = overrides.http;
  }
  if (overrides.logs !== undefined) {
    effects.logs = overrides.logs;
  }
  if (overrides.identityWrites !== undefined) {
    effects.identityWrites = overrides.identityWrites;
  }
  if (overrides.evidence !== undefined) {
    effects.evidence = overrides.evidence;
  }
  return effects;
}

export function bucket(initial: JsonObject, final: JsonObject): StateBucket {
  return { initial, final };
}
