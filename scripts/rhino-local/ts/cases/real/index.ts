import type { Given } from "../../src/case/types.ts";
import { hiddenValue, realCase, type BlockedBy, type RealEntry } from "./load.ts";

/**
 * Parse-error fixtures never reach a binding. The case format has no
 * `compile_error` channel; live recorded HTTP 401 / no HiddenValueCallback.
 * The JVM runner reports `compile_error` with Rhino's message.
 */
function parseError(message: string): BlockedBy {
  return { method: "(parse)", throw: message };
}

/**
 * Legacy emit via `JavaImporter` + `Action.send`. Measured 2026-09-12 against
 * the local JVM runner after `given.callbacks: []` unblocked `isEmpty`.
 * `frJava.Action` is undefined because the AM classes are not on the
 * classpath, so the exact throw is Rhino's `Cannot call method "send"`.
 */
function actionSend(line: string): BlockedBy {
  return {
    method: "Action.send",
    throw: `TypeError: Cannot call method "send" of undefined (${line})`,
  };
}

/** First-visit seed so `callbacks.isEmpty()` is `true` instead of a missing-fixture throw. */
function firstVisit(given?: Given): Given {
  return { ...(given ?? {}), callbacks: given?.callbacks ?? [] };
}

const ALICE_ID = "00000000-0000-0000-0000-000000000000";
const ALICE_MANAGER_ID = "00000000-0000-0000-0000-000000000001";
const BOB_ID = "00000000-0000-0000-0000-000000000002";

const aliceUser = {
  _id: ALICE_ID,
  userName: "alice",
  givenName: "Alice",
  sn: "Example",
  cn: "Alice Example",
  mail: "alice@example.com",
  telephoneNumber: "+10000000000",
  inetUserStatus: "active",
  "fr-idm-uuid": ALICE_ID,
  "fr-idm-custom-attrs": "{}",
};

const identityGiven: Given = {
  managed: {
    "managed/alpha_user": [aliceUser],
  },
};

const managerSwapGiven: Given = {
  managed: {
    "managed/alpha_user": [
      {
        ...aliceUser,
        _id: ALICE_MANAGER_ID,
        "fr-idm-uuid": ALICE_MANAGER_ID,
        userName: "alice",
        "fr-idm-managed-user-manager": BOB_ID,
      },
      {
        ...aliceUser,
        _id: BOB_ID,
        "fr-idm-uuid": BOB_ID,
        userName: "bob",
        givenName: "Bob",
        manager: ALICE_MANAGER_ID,
      },
    ],
  },
};

function entry(
  kind: "nextgen" | "legacy",
  name: string,
  originFile: string,
  expectPayload: Record<string, unknown>,
  extras: { given?: Given; blocked?: BlockedBy } = {}
): RealEntry {
  const init: {
    name: string;
    kind: "nextgen" | "legacy";
    origin: string;
    expect: { outcome: string; callbacks: ReturnType<typeof hiddenValue> };
    given: Given;
    blocked?: BlockedBy;
  } = {
    name,
    kind,
    origin: `scripts/rhino-script-tester/${originFile}`,
    expect: {
      outcome: "ok",
      callbacks: hiddenValue(expectPayload),
    },
    given: firstVisit(extras.given),
  };
  if (extras.blocked !== undefined) {
    init.blocked = extras.blocked;
  }
  return realCase(init);
}

function ng(
  name: string,
  originFile: string,
  payload: Record<string, unknown>,
  extras: { given?: Given; blocked?: BlockedBy } = {}
): RealEntry {
  return entry("nextgen", name, originFile, { ok: true, feature: name, ...payload }, extras);
}

function ngFeature(
  name: string,
  originFile: string,
  feature: string,
  payload: Record<string, unknown>,
  extras: { given?: Given; blocked?: BlockedBy } = {}
): RealEntry {
  return entry("nextgen", name, originFile, { ok: true, feature, ...payload }, extras);
}

function legacy(
  name: string,
  originFile: string,
  payload: Record<string, unknown>,
  extras: { given?: Given; blocked?: BlockedBy } = {}
): RealEntry {
  return entry("legacy", name, originFile, { ok: true, feature: name, ...payload }, extras);
}

const BINDINGS_AVAILABILITY_VALUE = JSON.stringify({
  require: "function",
  openidm: "object",
  httpClient: "object",
  utils: "object",
  logger: "object",
  idRepository: "object",
  nodeState: "object",
  action: "object",
  callbacks: "object",
  callbacksBuilder: "object",
  requestHeaders: "object",
  requestParameters: "object",
  requestCookies: "object",
  sharedState: "undefined",
  transientState: "undefined",
  realm: "string",
  systemEnv: "object",
  scriptName: "string",
  secrets: "object",
  existingSession: "undefined",
  resumedFromSuspend: "boolean",
  JavaImporter: "function",
  console: "undefined",
  process: "undefined",
  Buffer: "undefined",
  setTimeout: "undefined",
});

const ES2015_METHODS_VALUE = [
  { name: "Array.includes", ok: true, value: "true" },
  { name: "Array.find", ok: true, value: "2" },
  { name: "Array.from", ok: true, value: "a,b" },
  { name: "String.includes", ok: true, value: "true" },
  { name: "String.startsWith", ok: true, value: "true" },
  { name: "String.endsWith", ok: true, value: "true" },
  { name: "String.repeat", ok: true, value: "abab" },
  { name: "Object.assign", ok: true, value: '{"a":1,"b":2}' },
  { name: "Object.keys", ok: true, value: "a,b" },
];

const ES2015_GLOBALS_VALUE = [
  { name: "typeof Map", ok: true, value: "undefined" },
  { name: "typeof Set", ok: true, value: "undefined" },
  { name: "typeof WeakMap", ok: true, value: "undefined" },
  { name: "typeof WeakSet", ok: true, value: "undefined" },
  { name: "typeof Symbol", ok: true, value: "undefined" },
  { name: "typeof Proxy", ok: true, value: "undefined" },
  { name: "typeof Reflect", ok: true, value: "undefined" },
  { name: "typeof Promise", ok: true, value: "undefined" },
  { name: "typeof JSON", ok: true, value: "object" },
  {
    name: "new Map + set/get/size",
    ok: false,
    error: 'ReferenceError: "Map" is not defined.',
  },
  {
    name: "new Set + add/has/size",
    ok: false,
    error: 'ReferenceError: "Set" is not defined.',
  },
  { name: "object-as-map fallback", ok: true, value: "1:1" },
];

const STRING_NORMALIZE_RESULTS = [
  { name: "normalize-exists", ok: true, value: "function" },
  { name: "nfd-decompose", ok: true, value: "2" },
  { name: "nfd-fold-eacute", ok: true, value: "Jose" },
  { name: "nfd-fold-stacked", ok: true, value: "Nguyen" },
  { name: "nfc-compose", ok: true, value: "true" },
  { name: "lib-context", ok: true, value: "Jose|Nguyen|1" },
];

/**
 * Next-gen and legacy scripted-decision probes copied from
 * `scripts/rhino-script-tester/`. Expects are the live-recorded outcomes from
 * `docs/api/12-script-bindings-matrix.md` and `docs/api/14-am-identity-attributes.md`
 * (and, for language rows, the values the fixture itself computes).
 *
 * `blocked` is the first throw against today's overlay. Do not delete a case
 * when a binding lands — clear `blocked` so it becomes a pass assertion.
 * First-visit cases seed `given.callbacks: []` so `isEmpty` is a real
 * boolean, not a missing-fixture throw.
 */
export const realCases: RealEntry[] = [
  ng("arrow-function", "fixtures/arrow-function.script.js", {
    value: "42,42",
  }),
  ng("template-literal", "fixtures/template-literal.script.js", {
    value: "hi world 42",
  }),
  ng("const-in-function", "fixtures/const-in-function.script.js", {
    value: "const-in-function-ok",
  }),
  ng("const-uniq-across-blocks", "fixtures/const-uniq-across-blocks.script.js", {
    value: "first,second",
  }),
  ng("es2015-methods", "fixtures/es2015-methods.script.js", {
    value: ES2015_METHODS_VALUE,
  }),
  ng("es2015-globals", "fixtures/es2015-globals.script.js", {
    value: ES2015_GLOBALS_VALUE,
  }),
  ng("bindings-availability", "fixtures/bindings-availability.script.js", {
    value: BINDINGS_AVAILABILITY_VALUE,
  }),

  // Silent-data bugs, live-recorded values. Top-level `const` stringifies
  // without a `value` key because JSON.stringify drops `undefined`.
  realCase({
    name: "const-top-level",
    kind: "nextgen",
    origin: "scripts/rhino-script-tester/fixtures/const-top-level.script.js",
    expect: {
      outcome: "ok",
      callbacks: hiddenValue({ ok: true, feature: "const-top-level" }),
    },
    given: firstVisit(),
  }),
  ng("const-in-loop-body", "fixtures/const-in-loop-body.script.js", {
    value: ",,",
  }),
  ng(
    "const-in-nested-loop-block",
    "fixtures/const-in-nested-loop-block.script.js",
    { value: ",," }
  ),
  ng("const-in-while-body", "fixtures/const-in-while-body.script.js", {
    value: ",,",
  }),
  ng("const-in-do-while-body", "fixtures/const-in-do-while-body.script.js", {
    value: ",,",
  }),
  ng(
    "const-in-loop-in-function",
    "fixtures/const-in-loop-in-function.script.js",
    { value: "0,0,0" }
  ),

  // Parse errors: live HTTP 401 / no callback. Case format cannot say that;
  // blocked records the JVM compile_error. Expect is what the script would
  // emit if it parsed — it is not a live outcome.
  ng("const-dup-across-blocks", "fixtures/const-dup-across-blocks.script.js", {
    value: "first,second",
  }, {
    blocked: parseError(
      "compile_error: TypeError: redeclaration of const dup. (const-dup-across-blocks#21)"
    ),
  }),
  ng("const-in-for-init", "fixtures/const-in-for-init.script.js", {
    value: "ok",
  }, {
    blocked: parseError(
      "compile_error: syntax error (const-in-for-init#15)"
    ),
  }),
  ng("const-in-for-in", "fixtures/const-in-for-in.script.js", {
    value: "ok",
  }, {
    blocked: parseError("compile_error: syntax error (const-in-for-in#14)"),
  }),
  ng("const-in-for-of", "fixtures/const-in-for-of.script.js", {
    value: "ok",
  }, {
    blocked: parseError("compile_error: syntax error (const-in-for-of#14)"),
  }),
  ng("for-of-var", "fixtures/for-of-var.script.js", { value: "ok" }, {
    blocked: parseError(
      "compile_error: missing ; after for-loop initializer (for-of-var#34)"
    ),
  }),
  ng("object-shorthand", "fixtures/object-shorthand.script.js", {
    value: "ok",
  }, {
    blocked: parseError(
      "compile_error: missing : after property id (object-shorthand#14)"
    ),
  }),
  ng("destructuring-object", "fixtures/destructuring-object.script.js", {
    value: "ok",
  }, {
    blocked: parseError(
      "compile_error: missing : after property id (destructuring-object#13)"
    ),
  }),
  ng("default-params", "fixtures/default-params.script.js", { value: "ok" }, {
    blocked: parseError(
      "compile_error: missing ) after formal parameters (default-params#11)"
    ),
  }),
  ngFeature(
    "rhino-let-behaviour",
    "scripts/rhino-let-behaviour.script.js",
    "aic-rhino-let-probe",
    {},
    {
      blocked: parseError(
        "compile_error: missing ; before statement (rhino-let-behaviour#10)"
      ),
    }
  ),

  realCase({
    name: "rhino-var-control",
    kind: "nextgen",
    origin: "scripts/rhino-script-tester/scripts/rhino-var-control.script.js",
    expect: {
      outcome: "ok",
      callbacks: hiddenValue({
        ok: true,
        result: {
          marker: "aic-rhino-var-control",
          tests: [
            { name: "topLevelVar", value: "top" },
            { name: "blockScopedVar", value: "block" },
            { name: "functionVar", value: "function" },
            { name: "forVar", value: "0,1,2" },
          ],
        },
      }),
    },
    given: firstVisit(),
  }),

  ng("enum-callbacks-utils", "fixtures/enum-callbacks-utils.script.js", {
    // Live enumerates Java members via for-in. The exact name list is in
    // the probe payload, not committed as JSON; the case asserts the
    // successful emit shape. A local-vs-live disagreement on enumerability
    // of JS mock methods is a predicted fidelity gap (see the gap report).
  }),
  ng("enum-utils-sub", "fixtures/enum-utils-sub.script.js", {}),

  ng("logger-placeholders", "fixtures/logger-placeholders.script.js", {
    calls: {
      A1: "ok",
      A2: "ok",
      A3: "ok",
      A4: "ok",
      A5: "ok",
      B1: "ok",
      B2: "ok",
      B3: "ok",
      C1: "ok",
      D1: "ok",
      E0: "ok",
      E1: "ok",
      E2: "ok",
      E3: "ok",
      F1: "ok",
      F2: "ok",
    },
  }),

  ng("request-multivalue", "fixtures/request-multivalue.script.js", {}, {
    given: {
      requestHeaders: {
        "x-aic-probe": ["alpha", "bravo"],
        "x-aic-probe-joined": ["alpha,bravo"],
      },
      requestParameters: {
        probeq: ["alpha", "bravo"],
        probeqjoined: ["alpha,bravo"],
      },
    },
  }),
  ng("request-headers-dump", "fixtures/request-headers-dump.script.js", {}, {
    given: {
      requestHeaders: { "x-aic-probe": ["alpha"] },
      requestParameters: { probeq: ["alpha"] },
      requestCookies: { probe: "1" },
    },
  }),

  ng("httpclient-body-coercion", "fixtures/httpclient-body-coercion.script.js", {}, {
    given: {
      http: [
        {
          match: { url: "https://httpbin.org/post", method: "POST" },
          reply: {
            status: 200,
            body: {
              data: '{"intOne":1.0,"intZero":0.0,"negInt":-5.0,"bigInt":1000000.0,"floatVal":1.5,"undefField":null,"nullField":null,"strField":"s","boolField":true,"nested":{"u":null,"n":null,"i":2.0},"arr":[1.0,null,3.0]}',
            },
          },
        },
      ],
    },
  }),

  ng("java-collections", "fixtures/java-collections.script.js", {}),
  ng("java-class-shutter", "fixtures/java-class-shutter.script.js", {}),
  ng("for-each-java-collection", "fixtures/for-each-java-collection.script.js", {}),

  ng("string-normalize", "fixtures/string-normalize.script.js", {
    results: STRING_NORMALIZE_RESULTS,
  }),

  ngFeature(
    "lib-array-fill-consumer",
    "fixtures/lib-array-fill-consumer.script.js",
    "lib-array-fill-from",
    { fill: "false,false,false", from: "false,false,false" }
  ),
  ngFeature(
    "lib-const-consumer",
    "fixtures/lib-const-consumer.script.js",
    "lib-top-const",
    {}
  ),
  ngFeature(
    "lib-const-loop-consumer",
    "fixtures/lib-const-loop-consumer.script.js",
    "lib-const-loop-in-function",
    {}
  ),
  ngFeature(
    "lib-es2015-globals-consumer",
    "fixtures/lib-es2015-globals-consumer.script.js",
    "lib-es2015-globals",
    {}
  ),
  ngFeature(
    "lib-java-collections-consumer",
    "fixtures/lib-java-collections-consumer.script.js",
    "lib-java-collections",
    {}
  ),
  ngFeature(
    "lib-openidm-read-consumer",
    "fixtures/lib-openidm-read-consumer.script.js",
    "lib-openidm-read",
    {},
    {
      given: {
        managed: {
          "managed/alpha_name_variant": [
            {
              _id: "alice_bob",
              nameA: "alice",
              nameB: "bob",
              score: 1,
              imputed: false,
            },
          ],
        },
      },
    }
  ),
  ngFeature(
    "lib-openidm-miss-consumer",
    "fixtures/lib-openidm-miss-consumer.script.js",
    "lib-openidm-miss",
    {},
    {
      given: {
        managed: {
          "managed/idr_name_variants": [],
          "managed/idr_name_variant_discrepancies": [],
        },
      },
    }
  ),

  ng("identity-resolve-diag", "fixtures/identity-resolve-diag.script.js", {
    // Live (docs/api/14): UUID resolves; userName and amadmin do not
    // (`this.amIdentity is null`). Placeholders stand in for the sandbox
    // test user. Local getIdentity currently also matches userName — a
    // predicted fidelity gap, recorded in docs/rhino-local-gaps.md.
    value: JSON.stringify({
      alice: "attr-error",
      [ALICE_ID]: "ok givenName-size=1",
      amadmin: "attr-error",
      idRepository_typeof: "object",
      nodeState_username: "null",
    }),
  }, { given: identityGiven }),
  ng("identity-attr-mapping", "fixtures/identity-attr-mapping.script.js", {
    value: JSON.stringify({
      user: ALICE_ID,
      counts: {
        uid: 1,
        cn: 1,
        givenName: 1,
        sn: 1,
        mail: 1,
        displayName: 0,
        description: 0,
        telephoneNumber: 1,
        l: 0,
        st: 0,
        co: 0,
        postalCode: 0,
        inetUserStatus: 1,
        "fr-idm-uuid": 1,
        "fr-idm-managed-user-manager": 0,
        manager: 0,
        "fr-idm-managed-user-roles": 0,
        "fr-idm-managed-application-member": 0,
        "fr-idm-consentedMapping": 0,
        "fr-idm-custom-attrs": 1,
        frGivenName: 0,
        givenNameXYZ: 0,
      },
      customKeys: [],
    }),
  }, { given: identityGiven }),
  ng(
    "identity-getattribute-shape",
    "fixtures/identity-getattribute-shape.script.js",
    {},
    { given: identityGiven }
  ),
  ng("identity-enum-attrs", "fixtures/identity-enum-attrs.script.js", {}, {
    given: managerSwapGiven,
  }),
  ng("identity-manager-swap", "fixtures/identity-manager-swap.script.js", {
    value: JSON.stringify({
      A_frManager: 1,
      A_manager: 0,
      B_frManager: 0,
      B_manager: 1,
    }),
  }, { given: managerSwapGiven }),

  // Legacy engine. Three of these take a next-gen `callbacksBuilder` fallback
  // because the overlay still installs that binding; they run and miss the
  // live typeof dump. The other four have no fallback and throw on
  // `Action.send` (`frJava.Action` is undefined).
  legacy("legacy-bindings", "fixtures-legacy/legacy-bindings.script.js", {
    value: JSON.stringify({
      nodeState: "object",
      sharedState: "object",
      transientState: "object",
      callbacks: "object",
      callbacksBuilder: "undefined",
      action: "undefined",
      idRepository: "object",
      openidm: "undefined",
      httpClient: "object",
      utils: "undefined",
      requestHeaders: "object",
      requestParameters: "object",
      requestCookies: "undefined",
      existingSession: "undefined",
      resumedFromSuspend: "boolean",
      secrets: "object",
      JavaImporter: "function",
      logger: "object",
      realm: "string",
      systemEnv: "object",
      scriptName: "string",
    }),
  }),
  legacy(
    "legacy-es2015-globals",
    "fixtures-legacy/legacy-es2015-globals.script.js",
    {}
  ),
  legacy(
    "legacy-idrepository-methods",
    "fixtures-legacy/legacy-idrepository-methods.script.js",
    {
      idRepository: {
        getIdentity: "function",
        getAttribute: "function",
        setAttribute: "function",
        addAttribute: "function",
      },
    },
    { blocked: actionSend("legacy-idrepository-methods#13") }
  ),
  legacy(
    "legacy-nodestate-logger",
    "fixtures-legacy/legacy-nodestate-logger.script.js",
    {},
    { blocked: actionSend("legacy-nodestate-logger#15") }
  ),
  legacy(
    "legacy-logger-args",
    "fixtures-legacy/legacy-logger-args.script.js",
    {},
    { blocked: actionSend("legacy-logger-args#33") }
  ),
  legacy(
    "legacy-logger-levels",
    "fixtures-legacy/legacy-logger-levels.script.js",
    {},
    { blocked: actionSend("legacy-logger-levels#27") }
  ),
  legacy(
    "legacy-request-multivalue",
    "fixtures-legacy/legacy-request-multivalue.script.js",
    {},
    {
      given: {
        engine: "legacy",
        requestHeaders: {
          "x-aic-probe": ["alpha", "bravo"],
        },
        requestParameters: {
          probeq: ["alpha", "bravo"],
        },
      },
    }
  ),
];

export const runnableCases = realCases.filter((entry) => entry.blocked === undefined);
export const blockedCases = realCases.filter((entry) => entry.blocked !== undefined);
