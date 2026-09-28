import type { Given } from "../../src/case/types.ts";
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
    expect: { outcome: string; callbacks: ReturnType<typeof hiddenValue> };
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
];

export const runnableCases = realCases.filter(
  (entry) => entry.blocked === undefined && entry.gap === undefined
);
export const gapCases = realCases.filter(
  (entry) => entry.blocked === undefined && entry.gap !== undefined
);
export const blockedCases = realCases.filter((entry) => entry.blocked !== undefined);
