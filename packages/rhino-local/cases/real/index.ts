import type { Given, HttpExpect } from "../../src/case/types.ts";
import {
  hiddenValue,
  librarySource,
  realCase,
  type BlockedBy,
  type KnownGap,
  type RealEntry,
  type Rewrite,
} from "./load.ts";

/**
 * Parse-error fixtures never reach a binding. The case format has no
 * `compile_error` channel; live recorded HTTP 401 / no HiddenValueCallback.
 * The JVM runner reports `compile_error` with Rhino's message.
 */
function parseError(message: string): BlockedBy {
  return { method: "(parse)", throw: message };
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

/**
 * The identity probes name sandbox users by id and userName. These point them
 * at the placeholder records below instead, matching on the fixture's own
 * structure so no tenant identifier is repeated here.
 */
const probeUserIs = (id: string): Rewrite[] => [[/var PROBE_USER = "[^"]*";/, `var PROBE_USER = "${id}";`]];
const resolveDiagUsers: Rewrite[] = [
  [/"[^"]+",(\s*\/\/ userName \/ uid)/, '"alice",$1'],
  [/"[0-9a-f-]{36}",(\s*\/\/ managed-user uuid)/, `"${ALICE_ID}",$1`],
];
const reportAndManager: Rewrite[] = [
  [/var A = "[^"]*";/, `var A = "${ALICE_MANAGER_ID}";`],
  [/var B = "[^"]*";/, `var B = "${BOB_ID}";`],
];

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

interface Extras {
  given?: Given;
  blocked?: BlockedBy;
  libraries?: Record<string, string>;
  rewrites?: readonly Rewrite[];
  gap?: KnownGap;
  /** Requests the script makes; required alongside a `given.http` stub. */
  http?: HttpExpect[];
}

function entry(
  kind: "nextgen" | "legacy",
  name: string,
  originFile: string,
  expectPayload: Record<string, unknown>,
  extras: Extras = {}
): RealEntry {
  const init: {
    name: string;
    kind: "nextgen" | "legacy";
    origin: string;
    expect: {
      outcome: string;
      callbacks: ReturnType<typeof hiddenValue>;
      http?: HttpExpect[];
    };
    given: Given;
    blocked?: BlockedBy;
    libraries?: Record<string, string>;
    rewrites?: readonly Rewrite[];
    gap?: KnownGap;
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
  if (extras.http !== undefined) {
    init.expect.http = extras.http;
  }
  if (extras.blocked !== undefined) {
    init.blocked = extras.blocked;
  }
  if (extras.libraries !== undefined) {
    init.libraries = extras.libraries;
  }
  if (extras.rewrites !== undefined) {
    init.rewrites = extras.rewrites;
  }
  if (extras.gap !== undefined) {
    init.gap = extras.gap;
  }
  return realCase(init);
}

function ng(
  name: string,
  originFile: string,
  payload: Record<string, unknown>,
  extras: Extras = {}
): RealEntry {
  return entry("nextgen", name, originFile, { ok: true, feature: name, ...payload }, extras);
}

function ngFeature(
  name: string,
  originFile: string,
  feature: string,
  payload: Record<string, unknown>,
  extras: Extras = {}
): RealEntry {
  return entry("nextgen", name, originFile, { ok: true, feature, ...payload }, extras);
}

function legacy(
  name: string,
  originFile: string,
  payload: Record<string, unknown>,
  extras: Extras = {}
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
      "compile_error: TypeError: redeclaration of const dup. (const-dup-across-blocks#19)"
    ),
  }),
  ng("const-in-for-init", "fixtures/const-in-for-init.script.js", {
    value: "ok",
  }, {
    blocked: parseError(
      "compile_error: syntax error (const-in-for-init#13)"
    ),
  }),
  ng("const-in-for-in", "fixtures/const-in-for-in.script.js", {
    value: "ok",
  }, {
    blocked: parseError("compile_error: syntax error (const-in-for-in#12)"),
  }),
  ng("const-in-for-of", "fixtures/const-in-for-of.script.js", {
    value: "ok",
  }, {
    blocked: parseError("compile_error: syntax error (const-in-for-of#12)"),
  }),
  ng("for-of-var", "fixtures/for-of-var.script.js", { value: "ok" }, {
    blocked: parseError(
      "compile_error: missing ; after for-loop initializer (for-of-var#32)"
    ),
  }),
  ng("object-shorthand", "fixtures/object-shorthand.script.js", {
    value: "ok",
  }, {
    blocked: parseError(
      "compile_error: missing : after property id (object-shorthand#12)"
    ),
  }),
  ng("destructuring-object", "fixtures/destructuring-object.script.js", {
    value: "ok",
  }, {
    blocked: parseError(
      "compile_error: missing : after property id (destructuring-object#11)"
    ),
  }),
  ng("default-params", "fixtures/default-params.script.js", { value: "ok" }, {
    blocked: parseError(
      "compile_error: missing ) after formal parameters (default-params#9)"
    ),
  }),
  ngFeature(
    "rhino-let-behaviour",
    "scripts/rhino-let-behaviour.script.js",
    "aic-rhino-let-probe",
    {},
    {
      blocked: parseError(
        "compile_error: missing ; before statement (rhino-let-behaviour#8)"
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

  // Live payload, verbatim (probe run 2026-09-28).
  ng("enum-callbacks-utils", "fixtures/enum-callbacks-utils.script.js", {
    "callbacksBuilderEnum": [
      "booleanAttributeInputCallback",
      "callbacks",
      "choiceCallback",
      "class",
      "confirmationCallback",
      "consentMappingCallback",
      "deviceProfileCallback",
      "equals",
      "getCallbacks",
      "getClass",
      "hashCode",
      "hiddenValueCallback",
      "httpCallback",
      "idPCallback",
      "kbaCreateCallback",
      "languageCallback",
      "metadataCallback",
      "nameCallback",
      "notify",
      "notifyAll",
      "numberAttributeInputCallback",
      "passwordCallback",
      "pollingWaitCallback",
      "radioChoiceCallback",
      "redirectCallback",
      "scriptTextOutputCallback",
      "selectIdPCallback",
      "stringAttributeInputCallback",
      "suspendedTextOutputCallback",
      "termsAndConditionsCallback",
      "textInputCallback",
      "textOutputCallback",
      "toString",
      "validatedPasswordCallback",
      "validatedUsernameCallback",
      "wait",
      "x509CertificateCallback"
    ],
    "utilsEnum": [
      "base64",
      "base64url",
      "class",
      "crypto",
      "equals",
      "getClass",
      "hashCode",
      "notify",
      "notifyAll",
      "toString",
      "types",
      "wait"
    ],
    "candidateTypeof": {
      "nameCallback": "function",
      "passwordCallback": "function",
      "hiddenValueCallback": "function",
      "textInputCallback": "function",
      "textOutputCallback": "function",
      "scriptTextOutputCallback": "function",
      "confirmationCallback": "function",
      "choiceCallback": "function",
      "pollingWaitCallback": "function",
      "suspendedTextOutputCallback": "function",
      "stringAttributeInputCallback": "function",
      "numberAttributeInputCallback": "function",
      "booleanAttributeInputCallback": "function",
      "redirectCallback": "function",
      "metadataCallback": "function",
      "pingOneProtectInitializeCallback": "undefined",
      "pingOneProtectEvaluationCallback": "undefined",
      "deviceProfileCallback": "function",
      "selectIdPCallback": "function",
      "consentMappingCallback": "function",
      "kbaCreateCallback": "function",
      "termsAndConditionsCallback": "function",
      "validatedPasswordCallback": "function",
      "validatedUsernameCallback": "function"
    }
  }, {
    gap: {
      reason:
        "AIC's callbacksBuilder and utils are Java host objects, so for-in enumerates Java members (class, equals, getClass, hashCode, notify, notifyAll, wait; callbacksBuilder also callbacks/getCallbacks). The local mocks are plain JS objects and enumerate only their methods. Closing it means the mock exposing the same enumerable member set.",
      differs: ["callbacksBuilderEnum", "utilsEnum"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-28).
  ng("enum-utils-sub", "fixtures/enum-utils-sub.script.js", {
    "base64": [
      "atob",
      "btoa",
      "class",
      "decode",
      "decodeToBytes",
      "encode",
      "equals",
      "getClass",
      "hashCode",
      "notify",
      "notifyAll",
      "toString",
      "wait"
    ],
    "base64url": [
      "atob",
      "btoa",
      "class",
      "decode",
      "decodeToBytes",
      "encode",
      "equals",
      "getClass",
      "hashCode",
      "notify",
      "notifyAll",
      "toString",
      "wait"
    ],
    "crypto": [
      "checkBcrypt",
      "class",
      "equals",
      "getClass",
      "getRandomValues",
      "hashCode",
      "notify",
      "notifyAll",
      "randomUUID",
      "randomValues",
      "subtle",
      "toString",
      "wait"
    ],
    "types": [
      "bytesToString",
      "class",
      "equals",
      "getClass",
      "hashCode",
      "notify",
      "notifyAll",
      "stringToBytes",
      "toString",
      "wait"
    ],
    "arity": {
      "nameCallback": 0,
      "hiddenValueCallback": 0,
      "confirmationCallback": 0,
      "base64Encode": "function"
    }
  }, {
    gap: {
      reason:
        "Same Java-host-object enumeration as enum-callbacks-utils, for utils.base64/base64url/crypto/types (crypto on AIC also has checkBcrypt and randomValues). And AIC reports arity 0 for nameCallback/hiddenValueCallback/confirmationCallback (Java varargs bridges), where the JS mocks declare their parameters.",
      differs: ["base64", "base64url", "crypto", "types", "arity"],
    },
  }),

  // Live payload, verbatim (probe run 2026-09-28).
  ng("logger-placeholders", "fixtures/logger-placeholders.script.js", {
    "calls": {
      "A1": "ok",
      "A2": "ok",
      "A3": "ok",
      "A4": "ok",
      "A5": "ok",
      "B1": "ok",
      "B2": "ok",
      "B3": "ok",
      "C1": "ok",
      "D1": "ok",
      "E0-shape": "caught=InternalError: For input string: \"AICPROBE-not-a-number\" javaException=undefined rhinoException=object getMessage=undefined",
      "E0": "ok",
      "E1": "ok",
      "E2": "ok",
      "E3": "ok",
      "F1": "ok",
      "F2": "ok"
    }
  }),

  ng("request-multivalue", "fixtures/request-multivalue.script.js", {}, {
    gap: {
      reason:
        "Two differences, and the second makes the rest incomparable. (1) AIC's requestHeaders/requestParameters are Java maps: getClass() is refused with `InternalError: Access to Java class \"java.lang.Class\" is prohibited.` and String()/JSON show Java map formatting; the local mock is a JS object (`TypeError: Cannot find function getClass`). (2) The live probe run was a curl with its own headers, parameters and cookies, not this case's `given`, so the remaining keys were never measured against the same input. The live payload is not committed: it carries the tenant host and client-certificate headers.",
      differs: ["headers", "parameters"],
    },
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
    gap: {
      reason:
        "Two differences, and the second makes the rest incomparable. (1) AIC's requestHeaders/requestParameters are Java maps: getClass() is refused with `InternalError: Access to Java class \"java.lang.Class\" is prohibited.` and String()/JSON show Java map formatting; the local mock is a JS object (`TypeError: Cannot find function getClass`). (2) The live probe run was a curl with its own headers, parameters and cookies, not this case's `given`, so the remaining keys were never measured against the same input. The live payload is not committed: it carries the tenant host and client-certificate headers.",
      differs: ["headers", "parameters", "cookies"],
    },
    given: {
      requestHeaders: { "x-aic-probe": ["alpha"] },
      requestParameters: { probeq: ["alpha"] },
      requestCookies: { probe: "1" },
    },
  }),

  // Live payload, verbatim (probe run 2026-09-28).
  ng("httpclient-body-coercion", "fixtures/httpclient-body-coercion.script.js", {
    "javaIntAdded": true,
    "localStringify": "{\"intOne\":1,\"intZero\":0,\"negInt\":-5,\"bigInt\":1000000,\"floatVal\":1.5,\"nullField\":null,\"strField\":\"s\",\"boolField\":true,\"nested\":{\"n\":null,\"i\":2},\"arr\":[1,null,3],\"javaInt\":1,\"javaLong\":1}",
    "value": [
      {
        "name": "new java.lang.Integer(1)",
        "ok": true,
        "value": "1"
      },
      {
        "name": "new java.lang.Long(1)",
        "ok": true,
        "value": "1"
      },
      {
        "name": "new java.lang.Short(1)",
        "ok": true,
        "value": "1"
      },
      {
        "name": "new java.lang.Double(1)",
        "ok": true,
        "value": "1.0"
      },
      {
        "name": "java.lang.Integer.valueOf(1)",
        "ok": true,
        "value": "1"
      },
      {
        "name": "httpClient.send echo",
        "ok": true,
        "value": "{\"intOne\":1.0,\"intZero\":0.0,\"negInt\":-5.0,\"bigInt\":1000000.0,\"floatVal\":1.5,\"undefField\":null,\"nullField\":null,\"strField\":\"s\",\"boolField\":true,\"nested\":{\"u\":null,\"n\":null,\"i\":2.0},\"arr\":[1.0,null,3.0],\"javaInt\":1,\"javaLong\":1}"
      }
    ]
  }, {
    gap: {
      reason:
        "AIC serialises java.lang.Integer/Long body fields into the request (javaInt, javaLong appear in the echoed body); the local httpClient mock drops them. The echo reply in `given.http` is also a fixed string recorded without those fields, so closing this needs both the mock serialising Java boxed numbers and the reply echoing the request.",
      differs: ["value"],
    },
    http: [{ url: "https://httpbin.org/post", method: "POST" }],
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

  // Live payload, verbatim (probe run 2026-09-28).
  ng("java-collections", "fixtures/java-collections.script.js", {
    "value": [
      {
        "name": "typeof JavaImporter",
        "ok": true,
        "value": "function"
      },
      {
        "name": "typeof java",
        "ok": true,
        "value": "object"
      },
      {
        "name": "new java.util.HashSet",
        "ok": true,
        "value": "true:1"
      },
      {
        "name": "new java.util.ArrayList",
        "ok": true,
        "value": "a:1"
      },
      {
        "name": "new java.util.LinkedHashSet",
        "ok": true,
        "value": "1"
      },
      {
        "name": "new java.util.TreeSet",
        "ok": true,
        "value": "a:2"
      },
      {
        "name": "new java.util.HashMap",
        "ok": false,
        "error": "TypeError: [JavaPackage java.util.HashMap] is not a function, it is object."
      },
      {
        "name": "new java.util.LinkedHashMap",
        "ok": false,
        "error": "TypeError: [JavaPackage java.util.LinkedHashMap] is not a function, it is object."
      },
      {
        "name": "new java.util.TreeMap",
        "ok": false,
        "error": "TypeError: [JavaPackage java.util.TreeMap] is not a function, it is object."
      },
      {
        "name": "java.util.Collections.emptyMap",
        "ok": true,
        "value": "0"
      },
      {
        "name": "java.util.Collections.singletonMap",
        "ok": true,
        "value": "1:1"
      },
      {
        "name": "JavaImporter HashMap",
        "ok": false,
        "error": "TypeError: org.mozilla.javascript.Undefined@5f7e4dc0 is not a function, it is undefined."
      },
      {
        "name": "JavaImporter HashSet",
        "ok": true,
        "value": "1"
      }
    ]
  }, {
    gap: {
      reason:
        "`new ju.HashSet()` through `JavaImporter(java.util)` works on AIC (size 1) and throws locally. The HashMap one throws on both, with an identity hash in the message that differs per run.",
      differs: ["value"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-28).
  ng("java-class-shutter", "fixtures/java-class-shutter.script.js", {
    "value": [
      {
        "name": "CONTROL ArrayList.toArray index loop",
        "ok": true,
        "value": "a,b"
      },
      {
        "name": "CONTROL ArrayList.get(0)",
        "ok": true,
        "value": "a"
      },
      {
        "name": "CONTROL ArrayList.size()",
        "ok": true,
        "value": "2"
      },
      {
        "name": "ArrayList.iterator()",
        "ok": false,
        "error": "InternalError: Access to Java class \"java.util.ArrayList$Itr\" is prohibited. (AIC Rhino Let Probe#88)"
      },
      {
        "name": "HashSet.iterator()",
        "ok": true,
        "value": "a"
      },
      {
        "name": "LinkedHashSet.iterator()",
        "ok": false,
        "error": "InternalError: Access to Java class \"java.util.LinkedHashMap$LinkedKeyIterator\" is prohibited. (AIC Rhino Let Probe#101)"
      },
      {
        "name": "TreeSet.iterator()",
        "ok": false,
        "error": "InternalError: Access to Java class \"java.util.TreeMap$KeyIterator\" is prohibited. (AIC Rhino Let Probe#106)"
      },
      {
        "name": "Collections.singletonMap keySet().iterator()",
        "ok": true,
        "value": "a"
      },
      {
        "name": "Collections.unmodifiableList iterator()",
        "ok": true,
        "value": "a"
      },
      {
        "name": "JSON.stringify(ArrayList)",
        "ok": true,
        "value": "[\"a\",\"b\"]"
      },
      {
        "name": "JSON.stringify(HashSet)",
        "ok": true,
        "value": "[\"a\"]"
      },
      {
        "name": "JSON.stringify(LinkedHashSet)",
        "ok": true,
        "value": "[\"a\"]"
      },
      {
        "name": "JSON.stringify(singletonMap)",
        "ok": true,
        "value": "{\"a\":1}"
      },
      {
        "name": "String(ArrayList)",
        "ok": true,
        "value": "[a, b]"
      },
      {
        "name": "String(LinkedHashSet)",
        "ok": true,
        "value": "[a]"
      },
      {
        "name": "Object.keys(ArrayList)",
        "ok": true,
        "value": "0,1"
      },
      {
        "name": "for-in over ArrayList",
        "ok": true,
        "value": "2 keys"
      },
      {
        "name": "ArrayList.getClass().getName()",
        "ok": false,
        "error": "InternalError: Access to Java class \"java.lang.Class\" is prohibited. (AIC Rhino Let Probe#171)"
      }
    ]
  }),
  // Live payload, verbatim (probe run 2026-09-28).
  ng("for-each-java-collection", "fixtures/for-each-java-collection.script.js", {
    "value": [
      {
        "name": "for each over JS array",
        "ok": true,
        "value": "a,b"
      },
      {
        "name": "for each over java.util.ArrayList",
        "ok": true,
        "value": "a,b"
      },
      {
        "name": "for each over java.util.HashSet",
        "ok": false,
        "error": "InternalError: Access to Java class \"java.lang.Class\" is prohibited. (AIC Rhino Let Probe#64)"
      }
    ]
  }),

  ng("string-normalize", "fixtures/string-normalize.script.js", {
    results: STRING_NORMALIZE_RESULTS,
  }, {
    libraries: {
      "rhino-lib-normalize-probe": librarySource("lib-normalize-probe.lib.js"),
    },
  }),

  ngFeature(
    "lib-array-fill-consumer",
    "fixtures/lib-array-fill-consumer.script.js",
    "lib-array-fill-from",
    {
      fill: {
        ok: true,
        length: 3,
        joined: "false,false,false",
        allFalse: true,
      },
      from: {
        ok: true,
        length: 3,
        joined: "false,false,false",
        allFalse: true,
      },
    },
    {
      libraries: {
        "rhino-lib-array-fill-probe": librarySource("lib-array-fill-probe.lib.js"),
      },
    }
  ),
  ngFeature(
    "lib-const-consumer",
    "fixtures/lib-const-consumer.script.js",
    "lib-top-const",
    { fromConst: "lib-const-ok", fromVar: "lib-var-ok" },
    {
      libraries: {
        "rhino-lib-const-probe": librarySource("lib-const-probe.lib.js"),
      },
    }
  ),
  ngFeature(
    "lib-const-loop-consumer",
    "fixtures/lib-const-loop-consumer.script.js",
    "lib-const-loop-in-function",
    // Live (docs/api/12): loop-body const inside a function keeps the first
    // initializer; the correct series would be "0,2,4".
    { fromLoopConst: "0,0,0" },
    {
      libraries: {
        "rhino-lib-const-loop-probe": librarySource("lib-const-loop-probe.lib.js"),
      },
    }
  ),
  ngFeature(
    "lib-es2015-globals-consumer",
    "fixtures/lib-es2015-globals-consumer.script.js",
    "lib-es2015-globals",
    {
      globals: {
        Map: "undefined",
        Set: "undefined",
        WeakMap: "undefined",
        WeakSet: "undefined",
        Symbol: "undefined",
        Promise: "undefined",
        Proxy: "undefined",
        Reflect: "undefined",
        JSON: "object",
      },
      value: [
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
        { name: "object-as-set fallback", ok: true, value: "a,b" },
      ],
    },
    {
      libraries: {
        "rhino-lib-es2015-globals-probe": librarySource(
          "lib-es2015-globals-probe.lib.js"
        ),
      },
    }
  ),
  ngFeature(
    "lib-java-collections-consumer",
    "fixtures/lib-java-collections-consumer.script.js",
    "lib-java-collections",
    // Live payload, verbatim (probe run 2026-09-28).
    {
      "typeofJavaImporter": "function",
      "typeofJava": "object",
      "value": [
        {
          "name": "new java.util.HashSet",
          "ok": true,
          "value": "true:1"
        },
        {
          "name": "new java.util.ArrayList",
          "ok": true,
          "value": "a:1"
        },
        {
          "name": "new java.util.HashMap",
          "ok": false,
          "error": "TypeError: [JavaPackage java.util.HashMap] is not a function, it is object."
        },
        {
          "name": "java.util.Collections.singletonMap",
          "ok": true,
          "value": "1:1"
        }
      ]
    },
    {
      libraries: {
        "rhino-lib-java-collections-probe": librarySource(
          "lib-java-collections-probe.lib.js"
        ),
      },
    }
  ),
  ngFeature(
    "lib-openidm-read-consumer",
    "fixtures/lib-openidm-read-consumer.script.js",
    "lib-openidm-read",
    {
      fromLib: { nameA: "alice", nameB: "bob", score: 1, imputed: false },
      missResult: { threw: false, value: "null" },
    },
    {
      given: {
        managed: {
          "managed/alpha_name_variant": [
            {
              // Library body hardcodes this id (live seed name, not a person).
              _id: "aaron_erin",
              nameA: "alice",
              nameB: "bob",
              score: 1,
              imputed: false,
            },
          ],
        },
      },
      libraries: {
        "rhino-lib-openidm-read-probe": librarySource(
          "lib-openidm-read-probe.lib.js"
        ),
      },
    }
  ),
  ngFeature(
    "lib-openidm-miss-consumer",
    "fixtures/lib-openidm-miss-consumer.script.js",
    "lib-openidm-miss",
    {
      // Live (docs/api/10, re-measured 2026-09-28): missing record AND
      // missing type both return null.
      variantMiss: { threw: false, value: "null" },
      discrepancyMiss: { threw: false, value: "null" },
      unknownTypeMiss: { threw: false, value: "null" },
    },
    {
      gap: {
        reason:
          "openidm.read of a managed type that does not exist returns null on AIC. Locally an unseeded collection throws `no given.managed entry`, which is deliberate — it catches a case that forgot to seed — so closing this needs a way to declare a type absent rather than making every miss silent.",
        differs: ["unknownTypeMiss"],
      },
      given: {
        managed: {
          "managed/idr_name_variants": [],
          "managed/idr_name_variant_discrepancies": [],
        },
      },
      libraries: {
        "rhino-lib-openidm-miss-probe": librarySource(
          "lib-openidm-miss-probe.lib.js"
        ),
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
  }, {
    given: identityGiven,
    rewrites: resolveDiagUsers,
    gap: {
      reason:
        "AIC's idRepository.getIdentity resolves a managed-user UUID only; a userName (and amadmin) gives an identity whose getAttributeValues throws `this.amIdentity is null`. The local mock also resolves by userName.",
      differs: ["value"],
    },
  }),
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
  }, {
    given: identityGiven,
    rewrites: probeUserIs(ALICE_ID),
    gap: {
      reason:
        "AIC exposes the managed user's userName as the AM attribute `uid` (count 1); the local mock does not map it (count 0). Every other count agrees.",
      differs: ["value"],
    },
  }),
  ng(
    "identity-getattribute-shape",
    "fixtures/identity-getattribute-shape.script.js",
    // Live payload (probe run 2026-09-28); shape only, no attribute values.
    { value: "{\"getAttribute_customAttrs\":{\"call\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function getAttribute\"}},\"getAttributeValues_customAttrs\":{\"call\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"lengthProp\":\"number\",\"size\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"toArray\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"contains\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"includes\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function includes\"},\"get0\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"index0\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"charAt\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function charAt\"},\"substring\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function substring\"},\"lengthValue\":1,\"count\":1,\"toArrayLen\":1,\"asString\":\"[{}]\",\"keys\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"customKeys\":[]},\"getAttribute_mail\":{\"call\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function getAttribute\"}},\"getAttributeValues_mail\":{\"call\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"lengthProp\":\"number\",\"size\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"toArray\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"contains\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"includes\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function includes\"},\"get0\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"index0\":{\"ok\":true,\"undef\":false,\"isNull\":false},\"charAt\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function charAt\"},\"substring\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function substring\"},\"lengthValue\":1,\"count\":1,\"toArrayLen\":1},\"getAttribute_bogus\":{\"call\":{\"ok\":false,\"threw\":\"TypeError: Cannot find function getAttribute\"}}}" },
    {
      given: identityGiven,
      rewrites: probeUserIs(ALICE_ID),
      gap: {
        reason:
          "AIC's getAttributeValues returns a Java collection: it has toArray (length 1), has no includes, and String() prints `[{}]`. The local mock returns a JS array — includes, no toArray, `{}`.",
        differs: ["value"],
      },
    }
  ),
  ng("identity-enum-attrs", "fixtures/identity-enum-attrs.script.js", {}, {
    given: managerSwapGiven,
    rewrites: [reportAndManager[0]!],
    gap: {
      reason:
        "Not measured: the 2026-09-28 live run returned `err` for every count, i.e. getIdentity found no user — the probe's IDM setup users are gone from the sandbox. The live payload is therefore not committed; re-run the fixture's setup before comparing.",
      differs: ["value"],
    },
  }),
  ng("identity-manager-swap", "fixtures/identity-manager-swap.script.js", {
    value: JSON.stringify({
      A_frManager: 1,
      A_manager: 0,
      B_frManager: 0,
      B_manager: 1,
    }),
  }, { given: managerSwapGiven, rewrites: reportAndManager }),

  // Legacy engine. Emit is JavaImporter + Action.send; next-gen-only
  // bindings are undefined so scripts cannot take the callbacksBuilder
  // fallback.
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
    {
      value: [
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
        { name: "java.util.HashMap", ok: true, value: "1:1" },
      ],
    }
  ),

  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-utils", "fixtures/binding-utils.script.js", {
    "base64/string/0": {
      "ok": true,
      "value": {
        "encode": {
          "type": "string",
          "string": "aGVsbG8gd29ybGQ=",
          "array": false,
          "length": 16
        },
        "decode": {
          "type": "string",
          "string": "hello world",
          "array": false,
          "length": 11
        },
        "btoa": {
          "type": "string",
          "string": "aGVsbG8gd29ybGQ=",
          "array": false,
          "length": 16
        },
        "atob": {
          "type": "string",
          "string": "hello world",
          "array": false,
          "length": 11
        }
      }
    },
    "base64url/string/0": {
      "ok": true,
      "value": {
        "encode": {
          "type": "string",
          "string": "aGVsbG8gd29ybGQ",
          "array": false,
          "length": 15
        },
        "decode": {
          "type": "string",
          "string": "hello world",
          "array": false,
          "length": 11
        },
        "btoa": {
          "type": "string",
          "string": "aGVsbG8gd29ybGQ",
          "array": false,
          "length": 15
        },
        "atob": {
          "type": "string",
          "string": "hello world",
          "array": false,
          "length": 11
        }
      }
    },
    "base64/string/1": {
      "ok": true,
      "value": {
        "encode": {
          "type": "string",
          "string": "aMOpbGxvIOKckw==",
          "array": false,
          "length": 16
        },
        "decode": {
          "type": "string",
          "string": "héllo ✓",
          "array": false,
          "length": 7
        },
        "btoa": {
          "type": "string",
          "string": "aMOpbGxvIOKckw==",
          "array": false,
          "length": 16
        },
        "atob": {
          "type": "string",
          "string": "héllo ✓",
          "array": false,
          "length": 7
        }
      }
    },
    "base64url/string/1": {
      "ok": true,
      "value": {
        "encode": {
          "type": "string",
          "string": "aMOpbGxvIOKckw",
          "array": false,
          "length": 14
        },
        "decode": {
          "type": "string",
          "string": "héllo ✓",
          "array": false,
          "length": 7
        },
        "btoa": {
          "type": "string",
          "string": "aMOpbGxvIOKckw",
          "array": false,
          "length": 14
        },
        "atob": {
          "type": "string",
          "string": "héllo ✓",
          "array": false,
          "length": 7
        }
      }
    },
    "base64/string/2": {
      "ok": true,
      "value": {
        "encode": {
          "type": "string",
          "string": "w7vDvw==",
          "array": false,
          "length": 8
        },
        "decode": {
          "type": "string",
          "string": "ûÿ",
          "array": false,
          "length": 2
        },
        "btoa": {
          "type": "string",
          "string": "w7vDvw==",
          "array": false,
          "length": 8
        },
        "atob": {
          "type": "string",
          "string": "ûÿ",
          "array": false,
          "length": 2
        }
      }
    },
    "base64url/string/2": {
      "ok": true,
      "value": {
        "encode": {
          "type": "string",
          "string": "w7vDvw",
          "array": false,
          "length": 6
        },
        "decode": {
          "type": "string",
          "string": "ûÿ",
          "array": false,
          "length": 2
        },
        "btoa": {
          "type": "string",
          "string": "w7vDvw",
          "array": false,
          "length": 6
        },
        "atob": {
          "type": "string",
          "string": "ûÿ",
          "array": false,
          "length": 2
        }
      }
    },
    "base64/encode/bytes": {
      "ok": false,
      "error": "InternalError: Cannot convert org.mozilla.javascript.NativeArray@<hash> to byte[] (AIC Rhino Let Probe#82)"
    },
    "base64url/encode/bytes": {
      "ok": false,
      "error": "InternalError: Cannot convert org.mozilla.javascript.NativeArray@<hash> to byte[] (AIC Rhino Let Probe#87)"
    },
    "base64/decodeToBytes": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1"
        ],
        "length": 2
      }
    },
    "base64url/decodeToBytes": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1"
        ],
        "length": 2
      }
    },
    "base64/invalid": {
      "ok": false,
      "error": "InternalError: Illegal base64 character 25"
    },
    "base64url/invalid": {
      "ok": true,
      "value": null
    },
    "types/roundtrip": {
      "ok": true,
      "value": {
        "bytes": {
          "type": "object",
          "string": "[B@<hash>",
          "array": false,
          "keys": [
            "0",
            "1",
            "2",
            "3",
            "4",
            "5",
            "6",
            "7",
            "8",
            "9"
          ],
          "length": 10
        },
        "first": [
          {
            "value": 104,
            "type": "number"
          },
          {
            "value": -61,
            "type": "number"
          },
          {
            "value": -87,
            "type": "number"
          }
        ],
        "roundtrip": "héllo ✓"
      }
    },
    "crypto/randomUUID": {
      "ok": true,
      "value": {
        "type": "string",
        "length": 36,
        "uuid": true
      }
    },
    "crypto/getRandomValues": {
      "ok": true,
      "value": {
        "length": 4,
        "types": [
          "number",
          "number",
          "number",
          "number"
        ],
        "same": true,
        "array": true
      }
    },
    "crypto/checkBcrypt/right": {
      "ok": true,
      "value": true
    },
    "crypto/checkBcrypt/wrong": {
      "ok": true,
      "value": false
    },
    "subtle/digest/SHA-256": {
      "ok": true,
      "value": {
        "shape": {
          "type": "object",
          "string": "[B@<hash>",
          "array": false,
          "keys": [
            "0",
            "1",
            "10",
            "11",
            "12",
            "13",
            "14",
            "15",
            "16",
            "17",
            "18",
            "19",
            "2",
            "20",
            "21",
            "22",
            "23",
            "24",
            "25",
            "26",
            "27",
            "28",
            "29",
            "3",
            "30",
            "31",
            "4",
            "5",
            "6",
            "7",
            "8",
            "9"
          ],
          "length": 32
        },
        "hex": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
      }
    },
    "subtle/sign/HMAC/options": {
      "ok": false,
      "error": "InternalError: Unsupported hashing algorithm: [object Object]"
    },
    "subtle/sign/HMAC/string": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1",
          "10",
          "11",
          "12",
          "13",
          "14",
          "15",
          "16",
          "17",
          "18",
          "19",
          "2",
          "20",
          "21",
          "22",
          "23",
          "24",
          "25",
          "26",
          "27",
          "28",
          "29",
          "3",
          "30",
          "31",
          "4",
          "5",
          "6",
          "7",
          "8",
          "9"
        ],
        "length": 32
      }
    },
    "subtle/verify/right": {
      "ok": true,
      "value": true
    },
    "subtle/verify/tampered": {
      "ok": true,
      "value": false
    },
    "subtle/encrypt/string": {
      "ok": false,
      "error": "InternalError: Algorithm must be one of [AES, RSA]"
    },
    "subtle/decrypt/string": {
      "ok": false,
      "error": "InternalError: Algorithm must be one of [AES, RSA]"
    },
    "subtle/encrypt/decrypt/options": {
      "ok": false,
      "error": "InternalError: Algorithm must be one of [AES, RSA]"
    },
    "subtle/generateKey/object": {
      "ok": false,
      "error": "InternalError: Algorithm must be one of [AES, ECDSA, RSA, HMAC]"
    },
    "subtle/generateKey/string": {
      "ok": false,
      "error": "InternalError: Algorithm must be one of [AES, ECDSA, RSA, HMAC]"
    },
    "subtle/deriveKey/object": {
      "ok": false,
      "error": "InternalError: class org.mozilla.javascript.NativeArray cannot be cast to class [B (org.mozilla.javascript.NativeArray is in unnamed module of loader org.apache.catalina.loader.ParallelWebappClassLoader @<hash>; [B is in module java.base of loader 'bootstrap')"
    },
    "subtle/deriveKey/string": {
      "ok": false,
      "error": "InternalError: Salt must be provided for PBKDF2."
    }
  }, {
    gap: {
      reason:
        "utils.base64, utils.base64url, utils.types and utils.crypto are not mocked locally, so every probe records rhino-local's not-mocked error where AIC returns the value. Closing it means mocks for the tenant-independent members (encodings, byte conversion, UUID shape, getRandomValues, checkBcrypt).",
      differs: ["base64/string/0","base64url/string/0","base64/string/1","base64url/string/1","base64/string/2","base64url/string/2","base64/encode/bytes","base64url/encode/bytes","base64/decodeToBytes","base64url/decodeToBytes","base64/invalid","base64url/invalid","types/roundtrip","crypto/randomUUID","crypto/getRandomValues","crypto/checkBcrypt/right","crypto/checkBcrypt/wrong","subtle/digest/SHA-256","subtle/sign/HMAC/options","subtle/sign/HMAC/string","subtle/verify/right","subtle/verify/tampered","subtle/encrypt/string","subtle/decrypt/string","subtle/encrypt/decrypt/options","subtle/generateKey/object","subtle/generateKey/string","subtle/deriveKey/object","subtle/deriveKey/string"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-utils-subtle", "fixtures/binding-utils-subtle.script.js", {
    "generateKey/string/AES": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1",
          "10",
          "11",
          "12",
          "13",
          "14",
          "15",
          "16",
          "17",
          "18",
          "19",
          "2",
          "20",
          "21",
          "22",
          "23",
          "24",
          "25",
          "26",
          "27",
          "28",
          "29",
          "3",
          "30",
          "31",
          "4",
          "5",
          "6",
          "7",
          "8",
          "9"
        ],
        "length": 32
      }
    },
    "generateKey/object/AES": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1",
          "10",
          "11",
          "12",
          "13",
          "14",
          "15",
          "16",
          "17",
          "18",
          "19",
          "2",
          "20",
          "21",
          "22",
          "23",
          "24",
          "25",
          "26",
          "27",
          "28",
          "29",
          "3",
          "30",
          "31",
          "4",
          "5",
          "6",
          "7",
          "8",
          "9"
        ],
        "length": 32
      }
    },
    "generateKey/string/HMAC": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1",
          "10",
          "11",
          "12",
          "13",
          "14",
          "15",
          "16",
          "17",
          "18",
          "19",
          "2",
          "20",
          "21",
          "22",
          "23",
          "24",
          "25",
          "26",
          "27",
          "28",
          "29",
          "3",
          "30",
          "31",
          "4",
          "5",
          "6",
          "7",
          "8",
          "9"
        ],
        "length": 32
      }
    },
    "generateKey/object/HMAC": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1",
          "10",
          "11",
          "12",
          "13",
          "14",
          "15",
          "16",
          "17",
          "18",
          "19",
          "2",
          "20",
          "21",
          "22",
          "23",
          "24",
          "25",
          "26",
          "27",
          "28",
          "29",
          "3",
          "30",
          "31",
          "4",
          "5",
          "6",
          "7",
          "8",
          "9"
        ],
        "length": 32
      }
    },
    "generateKey/string/RSA": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"privateKey\": [B@<hash>, \"publicKey\": [B@<hash> }",
        "array": false,
        "keys": [
          "privateKey",
          "publicKey"
        ]
      }
    },
    "generateKey/object/RSA": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"privateKey\": [B@<hash>, \"publicKey\": [B@<hash> }",
        "array": false,
        "keys": [
          "privateKey",
          "publicKey"
        ]
      }
    },
    "generateKey/string/ECDSA": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"privateKey\": [B@<hash>, \"publicKey\": [B@<hash> }",
        "array": false,
        "keys": [
          "privateKey",
          "publicKey"
        ]
      }
    },
    "generateKey/object/ECDSA": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"privateKey\": [B@<hash>, \"publicKey\": [B@<hash> }",
        "array": false,
        "keys": [
          "privateKey",
          "publicKey"
        ]
      }
    },
    "encrypt/decrypt/AES/string": {
      "ok": true,
      "value": {
        "ciphertext": {
          "type": "object",
          "string": "[B@<hash>",
          "array": false,
          "keys": [
            "0",
            "1",
            "10",
            "11",
            "12",
            "13",
            "14",
            "15",
            "2",
            "3",
            "4",
            "5",
            "6",
            "7",
            "8",
            "9"
          ],
          "length": 16
        },
        "plaintext": "abc"
      }
    },
    "encrypt/decrypt/AES/object": {
      "ok": true,
      "value": {
        "ciphertext": {
          "type": "object",
          "string": "[B@<hash>",
          "array": false,
          "keys": [
            "0",
            "1",
            "10",
            "11",
            "12",
            "13",
            "14",
            "15",
            "2",
            "3",
            "4",
            "5",
            "6",
            "7",
            "8",
            "9"
          ],
          "length": 16
        },
        "plaintext": "abc"
      }
    },
    "encrypt/AES/generatedKey": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1",
          "10",
          "11",
          "12",
          "13",
          "14",
          "15",
          "2",
          "3",
          "4",
          "5",
          "6",
          "7",
          "8",
          "9"
        ],
        "length": 16
      }
    },
    "encrypt/RSA/generatedKey": {
      "ok": false,
      "error": "InternalError: Can't find method org.forgerock.openam.scripting.bindings.crypto.subtle.ScriptSubtleService.encrypt(string,org.forgerock.openam.scripting.javascript.MapScriptWrapper,[B). (AIC Rhino Let Probe#94)"
    },
    "sign/HMAC/object/hash-string": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1",
          "10",
          "11",
          "12",
          "13",
          "14",
          "15",
          "16",
          "17",
          "18",
          "19",
          "2",
          "20",
          "21",
          "22",
          "23",
          "24",
          "25",
          "26",
          "27",
          "28",
          "29",
          "3",
          "30",
          "31",
          "4",
          "5",
          "6",
          "7",
          "8",
          "9"
        ],
        "length": 32
      }
    },
    "sign/HMAC/string/hex": {
      "ok": true,
      "value": "9c196e32dc0175f86f4b1cb89289d6619de6bee699e4c378e68309ed97a1a6ab"
    },
    "deriveKey/object/PBKDF2": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[B@<hash>",
        "array": false,
        "keys": [
          "0",
          "1",
          "10",
          "11",
          "12",
          "13",
          "14",
          "15",
          "16",
          "17",
          "18",
          "19",
          "2",
          "20",
          "21",
          "22",
          "23",
          "24",
          "25",
          "26",
          "27",
          "28",
          "29",
          "3",
          "30",
          "31",
          "4",
          "5",
          "6",
          "7",
          "8",
          "9"
        ],
        "length": 32
      }
    },
    "deriveKey/string/PBKDF2": {
      "ok": false,
      "error": "InternalError: Salt must be provided for PBKDF2."
    },
    "encrypt/AES/hex-twice": {
      "ok": true,
      "value": [
        "1d25821c3e311eea2d4dd8633a25c1b5",
        "1d25821c3e311eea2d4dd8633a25c1b5"
      ]
    },
    "deriveKey/object/PBKDF2/hex": {
      "ok": true,
      "value": "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
    },
    "encrypt/decrypt/RSA/keyPair": {
      "ok": true,
      "value": {
        "ciphertext": {
          "type": "object",
          "string": "[B@<hash>",
          "array": false,
          "keys": [
            "0",
            "1",
            "10",
            "100",
            "101",
            "102",
            "103",
            "104",
            "105",
            "106",
            "107",
            "108",
            "109",
            "11",
            "110",
            "111",
            "112",
            "113",
            "114",
            "115",
            "116",
            "117",
            "118",
            "119",
            "12",
            "120",
            "121",
            "122",
            "123",
            "124",
            "125",
            "126",
            "127",
            "128",
            "129",
            "13",
            "130",
            "131",
            "132",
            "133",
            "134",
            "135",
            "136",
            "137",
            "138",
            "139",
            "14",
            "140",
            "141",
            "142",
            "143",
            "144",
            "145",
            "146",
            "147",
            "148",
            "149",
            "15",
            "150",
            "151",
            "152",
            "153",
            "154",
            "155",
            "156",
            "157",
            "158",
            "159",
            "16",
            "160",
            "161",
            "162",
            "163",
            "164",
            "165",
            "166",
            "167",
            "168",
            "169",
            "17",
            "170",
            "171",
            "172",
            "173",
            "174",
            "175",
            "176",
            "177",
            "178",
            "179",
            "18",
            "180",
            "181",
            "182",
            "183",
            "184",
            "185",
            "186",
            "187",
            "188",
            "189",
            "19",
            "190",
            "191",
            "192",
            "193",
            "194",
            "195",
            "196",
            "197",
            "198",
            "199",
            "2",
            "20",
            "200",
            "201",
            "202",
            "203",
            "204",
            "205",
            "206",
            "207",
            "208",
            "209",
            "21",
            "210",
            "211",
            "212",
            "213",
            "214",
            "215",
            "216",
            "217",
            "218",
            "219",
            "22",
            "220",
            "221",
            "222",
            "223",
            "224",
            "225",
            "226",
            "227",
            "228",
            "229",
            "23",
            "230",
            "231",
            "232",
            "233",
            "234",
            "235",
            "236",
            "237",
            "238",
            "239",
            "24",
            "240",
            "241",
            "242",
            "243",
            "244",
            "245",
            "246",
            "247",
            "248",
            "249",
            "25",
            "250",
            "251",
            "252",
            "253",
            "254",
            "255",
            "26",
            "27",
            "28",
            "29",
            "3",
            "30",
            "31",
            "32",
            "33",
            "34",
            "35",
            "36",
            "37",
            "38",
            "39",
            "4",
            "40",
            "41",
            "42",
            "43",
            "44",
            "45",
            "46",
            "47",
            "48",
            "49",
            "5",
            "50",
            "51",
            "52",
            "53",
            "54",
            "55",
            "56",
            "57",
            "58",
            "59",
            "6",
            "60",
            "61",
            "62",
            "63",
            "64",
            "65",
            "66",
            "67",
            "68",
            "69",
            "7",
            "70",
            "71",
            "72",
            "73",
            "74",
            "75",
            "76",
            "77",
            "78",
            "79",
            "8",
            "80",
            "81",
            "82",
            "83",
            "84",
            "85",
            "86",
            "87",
            "88",
            "89",
            "9",
            "90",
            "91",
            "92",
            "93",
            "94",
            "95",
            "96",
            "97",
            "98",
            "99"
          ],
          "length": 256
        },
        "plaintext": "abc"
      }
    },
    "sign/verify/ECDSA/keyPair": {
      "ok": true,
      "value": {
        "signature": {
          "type": "object",
          "string": "[B@<hash>",
          "array": false,
          "keys": [
            "0",
            "1",
            "10",
            "11",
            "12",
            "13",
            "14",
            "15",
            "16",
            "17",
            "18",
            "19",
            "2",
            "20",
            "21",
            "22",
            "23",
            "24",
            "25",
            "26",
            "27",
            "28",
            "29",
            "3",
            "30",
            "31",
            "32",
            "33",
            "34",
            "35",
            "36",
            "37",
            "38",
            "39",
            "4",
            "40",
            "41",
            "42",
            "43",
            "44",
            "45",
            "46",
            "47",
            "48",
            "49",
            "5",
            "50",
            "51",
            "52",
            "53",
            "54",
            "55",
            "56",
            "57",
            "58",
            "59",
            "6",
            "60",
            "61",
            "62",
            "63",
            "7",
            "8",
            "9"
          ],
          "length": 64
        },
        "verify": true
      }
    }
  }, {
    gap: {
      reason:
        "utils.crypto.subtle is not mocked locally, so every probe records rhino-local's not-mocked error where AIC computes digests, HMAC, AES-128-ECB, PBKDF2 and RSA/ECDSA key pairs, or rejects an algorithm name with its own message.",
      differs: ["generateKey/string/AES","generateKey/object/AES","generateKey/string/HMAC","generateKey/object/HMAC","generateKey/string/RSA","generateKey/object/RSA","generateKey/string/ECDSA","generateKey/object/ECDSA","encrypt/decrypt/AES/string","encrypt/decrypt/AES/object","encrypt/AES/generatedKey","encrypt/RSA/generatedKey","sign/HMAC/object/hash-string","sign/HMAC/string/hex","deriveKey/object/PBKDF2","deriveKey/string/PBKDF2","encrypt/AES/hex-twice","deriveKey/object/PBKDF2/hex","encrypt/decrypt/RSA/keyPair","sign/verify/ECDSA/keyPair"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-action", "fixtures/binding-action.script.js", {
    "withIdentifiedUser/1": {
      "ok": true
    },
    "withIdentifiedAgent/1": {
      "ok": true
    },
    "withHeader/1": {
      "ok": true
    },
    "withMaxSessionTime/1": {
      "ok": true
    },
    "withMaxIdleTime/1": {
      "ok": true
    },
    "putSessionProperty/2": {
      "ok": true
    },
    "withDescription/1": {
      "ok": true
    },
    "withStage/1": {
      "ok": true
    },
    "withErrorMessage/1": {
      "ok": true
    },
    "withLockoutMessage/1": {
      "ok": true
    },
    "removeSessionProperty/1": {
      "ok": true
    },
    "withMaxSessionTime/bad": {
      "ok": false,
      "error": "InternalError: Cannot convert x to java.lang.Integer (AIC Rhino Let Probe#96)"
    }
  }, {
    gap: {
      reason:
        "withMaxSessionTime(\"x\") returns the wrapper locally; AIC throws \"Cannot convert x to java.lang.Integer\".",
      differs: ["withMaxSessionTime/bad"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-services", "fixtures/binding-services.script.js", {
    "logger.getName": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "scripts.AUTHENTICATION_TREE_DECISION_NODE.<scriptId>.(AIC Rhino Let Probe)",
        "array": false,
        "length": 74
      }
    },
    "logger.isTraceEnabled": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "array": false
      }
    },
    "logger.isDebugEnabled": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "array": false
      }
    },
    "logger.isErrorEnabled": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "array": false
      }
    },
    "logger.isInfoEnabled": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "array": false
      }
    },
    "logger.isWarnEnabled": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "array": false
      }
    },
    "logger.trace": {
      "ok": true,
      "value": {
        "type": "undefined",
        "string": "undefined",
        "array": false
      }
    },
    "systemEnv.getProperty-1": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "null",
        "array": false
      }
    },
    "systemEnv.getProperty-2": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "default-two",
        "array": false,
        "length": 11
      }
    },
    "systemEnv.getProperty-3-string": {
      "ok": false,
      "error": "InternalError: Unsupported return type: java.lang.Integer"
    },
    "systemEnv.getProperty-3-class": {
      "ok": false,
      "error": "InternalError: Java class \"java.lang.Integer\" has no public instance field or method named \"class\". (AIC Rhino Let Probe#96)"
    },
    "systemEnv.getProperty-3-rhino-class-object": {
      "ok": false,
      "error": "InternalError: Property resolution failed"
    },
    "secrets.getGenericSecret": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.api.secrets.ScriptedSecretsException: Secret id rl-probe-absent not accessible"
    },
    "secrets.getDecryptionKey": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.api.secrets.ScriptedSecretsException: Secret id rl-probe-absent not accessible"
    },
    "secrets.getEncryptionKey": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.api.secrets.ScriptedSecretsException: Secret id rl-probe-absent not accessible"
    },
    "secrets.getSigningKey": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.api.secrets.ScriptedSecretsException: Secret id rl-probe-absent not accessible"
    },
    "secrets.getVerificationKey": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.api.secrets.ScriptedSecretsException: Secret id rl-probe-absent not accessible"
    },
    "cacheManager.named": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "null",
        "array": false
      }
    },
    "cacheManager.exists": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "array": false
      }
    },
    "journey.name": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "AIC-Rhino-Let-Probe",
        "array": false,
        "length": 19
      }
    },
    "journey.innerJourney": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "array": false
      }
    },
    "journey.mustRun": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "array": false
      }
    },
    "journey.identityResource": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "managed/alpha_user",
        "array": false,
        "length": 18
      }
    },
    "samlApplication.getApplicationId": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getApplicationId\" of null"
    },
    "samlApplication.getAuthnRequest": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getAuthnRequest\" of null"
    },
    "samlApplication.getIdpAttributes": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getIdpAttributes\" of null"
    },
    "samlApplication.getSpAttributes": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getSpAttributes\" of null"
    },
    "samlApplication.getFlowInitiator": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getFlowInitiator\" of null"
    },
    "samlApplication.getAssertion": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getAssertion\" of null"
    },
    "oauthApplication.getRequestProperties": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getRequestProperties\" of null"
    },
    "oauthApplication.getApplicationId": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getApplicationId\" of null"
    },
    "oauthApplication.getClientProperties": {
      "ok": false,
      "error": "TypeError: Cannot call method \"getClientProperties\" of null"
    },
    "jwtAssertion.generateJwt": {
      "ok": false,
      "error": "InternalError: Cannot invoke \"java.util.Map.get(Object)\" because \"jwtData\" is null"
    },
    "jwtValidator.validateJwtClaims": {
      "ok": false,
      "error": "InternalError: Can't find method org.forgerock.openam.scripting.bindings.JwtValidatorScriptWrapper.validateJwtClaims(string). (AIC Rhino Let Probe#219)"
    },
    "policy.evaluate": {
      "ok": false,
      "error": "InternalError: Invalid value subject"
    },
    "policy.evaluateTree": {
      "ok": false,
      "error": "InternalError: Invalid value subject"
    },
    "idRepository.getIdentity": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper@<hash>",
        "array": false,
        "keys": [
          "addAttribute",
          "attribute",
          "attributeValues",
          "class",
          "equals",
          "exists",
          "getAttributeValues",
          "getClass",
          "getName",
          "getUniversalId",
          "hashCode",
          "name",
          "notify",
          "notifyAll",
          "setAttribute",
          "store",
          "toString",
          "universalId",
          "wait"
        ]
      }
    },
    "samlApplication/typeof-enumeration": {
      "ok": true,
      "value": {
        "type": "object",
        "keys": "TypeError: Expected argument of type object, but instead had type object"
      }
    },
    "oauthApplication/typeof-enumeration": {
      "ok": true,
      "value": {
        "type": "object",
        "keys": "TypeError: Expected argument of type object, but instead had type object"
      }
    },
    "emailService/typeof-enumeration": {
      "ok": true,
      "value": {
        "type": "object",
        "keys": [
          "class",
          "equals",
          "getClass",
          "hashCode",
          "notify",
          "notifyAll",
          "send",
          "toString",
          "wait"
        ]
      }
    },
    "realm": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "/alpha",
        "array": false,
        "length": 6
      }
    },
    "scriptName": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "AIC Rhino Let Probe",
        "array": false,
        "length": 19
      }
    },
    "cookieName": {
      "ok": true,
      "value": "string"
    },
    "resumedFromSuspend": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "array": false
      }
    },
    "locales": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "org.forgerock.openam.auth.nodes.script.ScriptedLocalizedMessageImpl@<hash>",
        "array": false,
        "keys": [
          "class",
          "equals",
          "getClass",
          "getLocalizedMessage",
          "hashCode",
          "localizedMessage",
          "notify",
          "notifyAll",
          "toString",
          "wait"
        ]
      }
    },
    "systemEnv.getProperty-3/string": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "42",
        "array": false,
        "length": 2
      }
    },
    "systemEnv.getProperty-3/number": {
      "ok": true,
      "value": {
        "type": "number",
        "string": "42",
        "array": false
      }
    },
    "systemEnv.getProperty-3/boolean": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "array": false
      }
    },
    "systemEnv.getProperty-3/object": {
      "ok": false,
      "error": "InternalError: Property resolution failed"
    },
    "systemEnv.getProperty-3/array": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[42]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      }
    },
    "systemEnv.getProperty-3/list": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[42]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      }
    },
    "systemEnv.getProperty-3/map": {
      "ok": false,
      "error": "InternalError: Property resolution failed"
    },
    "systemEnv.getProperty-3/java.lang.String": {
      "ok": false,
      "error": "InternalError: Unsupported return type: java.lang.String"
    },
    "systemEnv.getProperty-3/java.lang.Boolean": {
      "ok": false,
      "error": "InternalError: Unsupported return type: java.lang.Boolean"
    },
    "systemEnv.getProperty-3/java.util.List": {
      "ok": false,
      "error": "InternalError: Unsupported return type: java.util.List"
    },
    "systemEnv.getProperty-3/class/Integer": {
      "ok": true,
      "value": {
        "type": "number",
        "string": "42",
        "array": false
      }
    },
    "systemEnv.getProperty-3/class/String": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "42",
        "array": false,
        "length": 2
      }
    },
    "systemEnv.getProperty-3/class/Boolean": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "array": false
      }
    },
    "systemEnv.getProperty-3/class/Double": {
      "ok": true,
      "value": {
        "type": "number",
        "string": "4.5",
        "array": false
      }
    },
    "systemEnv.getProperty-3/object/json": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"a\": 1 }",
        "array": false,
        "keys": [
          "a"
        ],
        "size": 1
      }
    },
    "systemEnv.getProperty-3/map/json": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"a\": 1 }",
        "array": false,
        "keys": [
          "a"
        ],
        "size": 1
      }
    },
    "systemEnv.getProperty-3/array/json": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[[\"a\", \"b\"]]",
        "array": false,
        "keys": [
          "0",
          "1"
        ],
        "length": 2,
        "size": 2
      }
    },
    "systemEnv.getProperty-3/array/csv": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[a, b]",
        "array": false,
        "keys": [
          "0",
          "1"
        ],
        "length": 2,
        "size": 2
      }
    },
    "systemEnv.getProperty-3/list/csv-spaced": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[a,  b]",
        "array": false,
        "keys": [
          "0",
          "1"
        ],
        "length": 2,
        "size": 2
      }
    },
    "systemEnv.getProperty-3/number/4.5": {
      "ok": true,
      "value": {
        "type": "number",
        "string": "4.5",
        "array": false
      }
    },
    "systemEnv.getProperty-3/number/not-a-number": {
      "ok": false,
      "error": "InternalError: Property resolution failed"
    },
    "systemEnv.getProperty-3/boolean/TRUE": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "array": false
      }
    },
    "systemEnv.getProperty-3/String": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "42",
        "array": false,
        "length": 2
      }
    },
    "systemEnv.getProperty-3/int": {
      "ok": false,
      "error": "InternalError: Unsupported return type: int"
    },
    "systemEnv.getProperty-3/integer": {
      "ok": true,
      "value": {
        "type": "number",
        "string": "42",
        "array": false
      }
    },
    "jwtAssertion.generateJwt/empty": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "null",
        "array": false
      }
    },
    "jwtAssertion.generateJwt/HS256": {
      "ok": false,
      "error": "InternalError: Missing argument"
    },
    "jwtAssertion.generateJwt/HS256/string-key": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "string/3 parts",
        "array": false,
        "length": 14
      }
    },
    "jwtValidator.validateJwtClaims/empty": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "null",
        "array": false,
        "length": 4
      }
    },
    "jwtValidator.validateJwtClaims/HS256": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "{\"keys\":[\"audience\",\"expirationTime\",\"issuedAt\",\"issuer\",\"jwtId\",\"subject\",\"type\"],\"issuer\":\"https://example.com\",\"subject\":\"probe\",\"audience\":[\"https://example.com\"],\"type\":\"JWT\"}",
        "array": false,
        "length": 180
      }
    },
    "policy.evaluate/claims/oauth2Scopes": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "[{\"resourceName\":\"https://example.com/\",\"attributes\":{},\"advices\":{},\"actions\":{}}]",
        "array": false,
        "length": 83
      }
    },
    "policy.evaluateTree/claims/oauth2Scopes": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "[{\"resourceName\":\"https://example.com/\",\"attributes\":{},\"advices\":{},\"actions\":{}}]",
        "array": false,
        "length": 83
      }
    },
    "policy.evaluate/ssoToken": {
      "ok": false,
      "error": "InternalError: Invalid value subject"
    },
    "policy.evaluate/jwt": {
      "ok": false,
      "error": "InternalError: Invalid value subject"
    },
    "policy.evaluate/claims": {
      "ok": false,
      "error": "InternalError: Unable to retrieve application under realm /alpha."
    }
  }, {
    gap: {
      reason:
        "Mostly not mocked locally: logger's level queries and getName, every systemEnv.getProperty overload but the one-argument form, secrets, cacheManager, journey, samlApplication/oauthApplication, jwtAssertion/jwtValidator, policy and emailService enumeration throw rhino-local's not-mocked error or return a JS-object shape. Several are tenant state (journey values, policy sets, a null application binding) and are expected to stay gaps.",
      differs: ["logger.getName","logger.isTraceEnabled","systemEnv.getProperty-1","systemEnv.getProperty-2","systemEnv.getProperty-3-string","systemEnv.getProperty-3-rhino-class-object","secrets.getGenericSecret","secrets.getDecryptionKey","secrets.getEncryptionKey","secrets.getSigningKey","secrets.getVerificationKey","cacheManager.named","cacheManager.exists","journey.name","journey.innerJourney","journey.mustRun","journey.identityResource","samlApplication.getApplicationId","samlApplication.getAuthnRequest","samlApplication.getIdpAttributes","samlApplication.getSpAttributes","samlApplication.getFlowInitiator","samlApplication.getAssertion","oauthApplication.getRequestProperties","oauthApplication.getApplicationId","oauthApplication.getClientProperties","jwtAssertion.generateJwt","jwtValidator.validateJwtClaims","policy.evaluate","policy.evaluateTree","idRepository.getIdentity","samlApplication/typeof-enumeration","oauthApplication/typeof-enumeration","emailService/typeof-enumeration","realm","scriptName","locales","systemEnv.getProperty-3/string","systemEnv.getProperty-3/number","systemEnv.getProperty-3/boolean","systemEnv.getProperty-3/object","systemEnv.getProperty-3/array","systemEnv.getProperty-3/list","systemEnv.getProperty-3/map","systemEnv.getProperty-3/java.lang.String","systemEnv.getProperty-3/java.lang.Boolean","systemEnv.getProperty-3/java.util.List","systemEnv.getProperty-3/class/Integer","systemEnv.getProperty-3/class/String","systemEnv.getProperty-3/class/Boolean","systemEnv.getProperty-3/class/Double","systemEnv.getProperty-3/object/json","systemEnv.getProperty-3/map/json","systemEnv.getProperty-3/array/json","systemEnv.getProperty-3/array/csv","systemEnv.getProperty-3/list/csv-spaced","systemEnv.getProperty-3/number/4.5","systemEnv.getProperty-3/number/not-a-number","systemEnv.getProperty-3/boolean/TRUE","systemEnv.getProperty-3/String","systemEnv.getProperty-3/int","systemEnv.getProperty-3/integer","jwtAssertion.generateJwt/empty","jwtAssertion.generateJwt/HS256","jwtAssertion.generateJwt/HS256/string-key","jwtValidator.validateJwtClaims/empty","jwtValidator.validateJwtClaims/HS256","policy.evaluate/claims/oauth2Scopes","policy.evaluateTree/claims/oauth2Scopes","policy.evaluate/ssoToken","policy.evaluate/jwt","policy.evaluate/claims"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-createuser", "fixtures/binding-createuser.script.js", {
    "createUser/2": {
      "ok": false,
      "error": "InternalError: User creation through identity repository is not allowed in this environment"
    },
    "createUser/3": {
      "ok": false,
      "error": "InternalError: class java.lang.String cannot be cast to class java.util.Collection (java.lang.String and java.util.Collection are in module java.base of loader 'bootstrap')"
    },
    "createUser/3/arrays": {
      "ok": false,
      "error": "InternalError: User creation through identity repository is not allowed in this environment"
    },
    "createUser/duplicate": {
      "ok": false,
      "error": "InternalError: User creation through identity repository is not allowed in this environment"
    }
  }, {
    gap: {
      reason:
        "idRepository.createUser is not mocked locally; this AIC environment refuses it (\"User creation through identity repository is not allowed in this environment\") after a ClassCastException for non-array attribute values.",
      differs: ["createUser/2","createUser/3","createUser/3/arrays","createUser/duplicate"],
    },
  }),
];

export const runnableCases = realCases.filter(
  (entry) => entry.blocked === undefined && entry.gap === undefined
);
export const gapCases = realCases.filter(
  (entry) => entry.blocked === undefined && entry.gap !== undefined
);
export const blockedCases = realCases.filter((entry) => entry.blocked !== undefined);
