import type { AllowUndeclared, Given, HttpExpect } from "../../src/case/types.ts";
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
  /**
   * Side-effect channels the live run did not capture. A probe's payload is
   * its measurement; pinning the local value of an uncaptured channel would
   * read as a live claim.
   */
  allowUndeclared?: AllowUndeclared;
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
      allowUndeclared?: AllowUndeclared;
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
  if (extras.allowUndeclared !== undefined) {
    init.expect.allowUndeclared = extras.allowUndeclared;
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
      "error": "InternalError: Cannot convert org.mozilla.javascript.NativeArray@<hash> to byte[] (AIC Rhino Let Probe#86)"
    },
    "base64url/encode/bytes": {
      "ok": false,
      "error": "InternalError: Cannot convert org.mozilla.javascript.NativeArray@<hash> to byte[] (AIC Rhino Let Probe#91)"
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
        "uuid": true,
        "v4": true
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
        "array": true,
        "int32": true,
        "anyNegative": true
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
        "Base64/UTF-8, UUID, random values and every subtle result and rejection match, through java/HostOps.java. Still different: encoding a JS array fails on both, but AIC suffixes its conversion error with the calling script's (name#line) and local with the mock's; checkBcrypt is unmocked (no bcrypt in the JDK, and no dependency added); PBKDF2 with a JS-array salt fails on both, but AIC's ClassCastException also names its webapp class loader.",
      differs: ["base64/encode/bytes","base64url/encode/bytes","crypto/checkBcrypt/right","crypto/checkBcrypt/wrong","subtle/deriveKey/object"],
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
      "error": "InternalError: Can't find method org.forgerock.openam.scripting.bindings.crypto.subtle.ScriptSubtleService.encrypt(string,org.forgerock.openam.scripting.javascript.MapScriptWrapper,[B). (AIC Rhino Let Probe#98)"
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
        "Digest, HMAC, AES-128-ECB, PBKDF2 and RSA/ECDSA key pairs match (AIC's vectors reproduced). Still different: RSA encrypt given the whole key-pair map fails on both, but AIC reports a Java overload miss naming its MapScriptWrapper and local a byte[] conversion error.",
      differs: ["encrypt/RSA/generatedKey"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-action", "fixtures/binding-action.script.js", {
    "withIdentifiedUser/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withIdentifiedAgent/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withHeader/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withMaxSessionTime/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withMaxIdleTime/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "putSessionProperty/2": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withDescription/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withStage/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withErrorMessage/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withLockoutMessage/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "removeSessionProperty/1": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withMaxSessionTime/bad": {
      "ok": false,
      "error": "InternalError: Cannot convert x to java.lang.Integer (AIC Rhino Let Probe#100)"
    },
    "withMaxSessionTime/numeric-string": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withMaxSessionTime/null": {
      "ok": false,
      "error": "InternalError: Can't find method org.forgerock.openam.auth.nodes.script.ActionWrapper.withMaxSessionTime(null). (AIC Rhino Let Probe#110)"
    },
    "withMaxSessionTime/fraction": {
      "ok": true,
      "type": "object",
      "string": "org.forgerock.openam.auth.nodes.script.ActionWrapper@<hash>",
      "same": true
    },
    "withMaxIdleTime/bad": {
      "ok": false,
      "error": "InternalError: Cannot convert x to java.lang.Integer (AIC Rhino Let Probe#120)"
    }
  }, {
    gap: {
      reason:
        "withMaxSessionTime/withMaxIdleTime reject \"x\" and null with the same messages locally, but AIC appends the calling script's \"(name#line)\" and a mock-thrown InternalError carries none.",
      differs: ["withMaxSessionTime/bad","withMaxSessionTime/null","withMaxIdleTime/bad"],
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
      "error": "InternalError: Java class \"java.lang.Integer\" has no public instance field or method named \"class\". (AIC Rhino Let Probe#100)"
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
      "error": "InternalError: Can't find method org.forgerock.openam.scripting.bindings.JwtValidatorScriptWrapper.validateJwtClaims(string). (AIC Rhino Let Probe#223)"
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
    given: {
      "realm": "/alpha",
      "scriptName": "AIC Rhino Let Probe",
      "esv": {},
      "secrets": {},
      "bindings": {
        "cacheManager": {},
        "journey": {
          "name": "AIC-Rhino-Let-Probe",
          "identityResource": "managed/alpha_user",
          "innerJourney": false,
          "mustRun": false
        }
      }
    },
    gap: {
      reason:
        "57/80 measured payload keys now match: ESV defaults and conversions, cacheManager, the seeded journey values and flags, null SAML/OAuth bindings, and logger.isTraceEnabled. Remaining gaps are tenant-backed JWT creation/validation and policy results; idRepository.getIdentity on an absent user (AIC returns its Java wrapper, which this mock does not model); JavaException wrappers for absent secrets; logger.getName includes the tenant script id; and Java reflection shapes for emailService enumeration and locales.",
      differs: ["logger.getName","secrets.getGenericSecret","secrets.getDecryptionKey","secrets.getEncryptionKey","secrets.getSigningKey","secrets.getVerificationKey","jwtAssertion.generateJwt","jwtValidator.validateJwtClaims","policy.evaluate","policy.evaluateTree","idRepository.getIdentity","emailService/typeof-enumeration","locales","jwtAssertion.generateJwt/empty","jwtAssertion.generateJwt/HS256","jwtAssertion.generateJwt/HS256/string-key","jwtValidator.validateJwtClaims/empty","jwtValidator.validateJwtClaims/HS256","policy.evaluate/claims/oauth2Scopes","policy.evaluateTree/claims/oauth2Scopes","policy.evaluate/ssoToken","policy.evaluate/jwt","policy.evaluate/claims"],
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
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-nodestate", "fixtures/binding-nodestate.script.js", {
    "putShared/returns-self": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "json": "true"
      }
    },
    "putTransient/returns-self": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "json": "true"
      }
    },
    "get/shared": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "s1",
        "json": "\"s1\""
      }
    },
    "get/transient": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "t1",
        "json": "\"t1\""
      }
    },
    "getObject/shared": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "s1",
        "json": "\"s1\""
      }
    },
    "isDefined/shared": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "json": "true"
      }
    },
    "isDefined/transient": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "json": "true"
      }
    },
    "isDefined/absent": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "json": "false"
      }
    },
    "get/absent": {
      "ok": true,
      "value": "null"
    },
    "getObject/absent": {
      "ok": true,
      "value": "null"
    },
    "putShared/object": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"a\":1,\"nested\":{\"b\":\"x\"},\"list\":[1,2]}"
      }
    },
    "getObject/object": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"a\": 1.0, \"nested\": { \"b\": \"x\" }, \"list\": [ 1.0, 2.0 ] }",
        "json": "{\"a\":1,\"nested\":{\"b\":\"x\"},\"list\":[1,2]}"
      }
    },
    "getObject/object/field": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "x",
        "json": "\"x\""
      }
    },
    "mergeShared/returns-self": {
      "ok": false,
      "error": "InternalError: State must not contain nested objects unless they are inside registered state containers: objectAttributes"
    },
    "mergeShared/after/replaced-or-deep": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"a\":1,\"nested\":{\"b\":\"x\"},\"list\":[1,2]}"
      }
    },
    "mergeShared/after/new-key": {
      "ok": true,
      "value": "null"
    },
    "mergeTransient/returns-self": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "json": "true"
      }
    },
    "mergeTransient/after": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "t2,mt",
        "json": "[\"t2\",\"mt\"]"
      }
    },
    "putShared/null": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "true,",
        "json": "[true,null]"
      }
    },
    "shadow/transient-over-shared": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "transient",
        "json": "\"transient\""
      }
    },
    "remove/returns-self": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "json": "false"
      }
    },
    "remove/after": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "false,",
        "json": "[false,null]"
      }
    },
    "remove/both-buckets": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "false,",
        "json": "[false,null]"
      }
    },
    "remove/absent": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "false",
        "json": "false"
      }
    },
    "mergeShared/flat": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "json": "true"
      }
    },
    "mergeShared/flat/after": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "f,3",
        "json": "[\"f\",3]"
      }
    },
    "keys": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"type\":\"object\",\"rl\":[\"rlFlat\",\"rlFlatNum\",\"rlMergedT\",\"rlNull\",\"rlObj\",\"rlTransient\"]}"
      }
    },
    "mergeShared/objectAttributes": {
      "ok": true,
      "value": {
        "type": "boolean",
        "string": "true",
        "json": "true"
      }
    },
    "mergeShared/objectAttributes/after": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"rlOa\": 1.0 }",
        "json": "{\"rlOa\":1}"
      }
    },
    "mergeTransient/nested": {
      "ok": false,
      "error": "InternalError: State must not contain nested objects unless they are inside registered state containers: objectAttributes"
    },
    "mergeTransient/nested/after": {
      "ok": true,
      "value": "null"
    },
    "getObject/object/methods": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "function,function,function",
        "json": "[\"function\",\"function\",\"function\"]"
      }
    }
  }, {
    allowUndeclared: {"sharedState":true,"transientState":true},
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-openidm-writes", "fixtures/binding-openidm-writes.script.js", {
    "cleanup/before": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: No Such Entry: The search base entry &#39;uid&#61;rl-probe-openidm-writes,ou&#61;role,o&#61;alpha,o&#61;root,ou&#61;identities&#39; does not exist"
    },
    "create/5": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes\",\"_rev\":\"<rev>\",\"name\":\"rl-a\",\"description\":\"rl-probe\"}"
      }
    },
    "create/duplicate": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Entry Already Exists: The entry &#39;uid&#61;rl-probe-openidm-writes,ou&#61;role,o&#61;alpha,o&#61;root,ou&#61;identities&#39; cannot be added because an entry with that name already exists"
    },
    "create/3": {
      "ok": true,
      "value": {
        "type": "string",
        "string": "string",
        "json": "\"string\""
      }
    },
    "create/fields": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes-fields\",\"_rev\":\"<rev>\",\"name\":\"rl-f\"}"
      }
    },
    "update/null-rev": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes\",\"_rev\":\"<rev>\",\"name\":\"rl-a2\",\"description\":\"rl-probe\"}"
      }
    },
    "update/after": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes\",\"_rev\":\"<rev>\",\"name\":\"rl-a2\",\"description\":\"rl-probe\"}"
      }
    },
    "update/wrong-rev": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: The resource could not be accessed because the expected version &#39;0&#39; does not match the current version &#39;<uuid>&#39;"
    },
    "patch/replace": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes\",\"_rev\":\"<rev>\",\"name\":\"rl-a2\",\"description\":\"rl-9\"}"
      }
    },
    "patch/add-unknown-field": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes\",\"_rev\":\"<rev>\",\"name\":\"rl-a2\",\"description\":\"rl-9\",\"rlNotInSchema\":\"x\"}"
      }
    },
    "patch/bad-operation": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: The request could not be processed because the provided content is not a valid JSON patch."
    },
    "patch/fields": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes\",\"_rev\":\"<rev>\",\"description\":\"rl-8\"}"
      }
    },
    "create/4": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes-4\",\"_rev\":\"<rev>\",\"name\":\"rl-4\",\"description\":\"rl-probe\"}"
      }
    },
    "update/4": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes-4\",\"_rev\":\"<rev>\",\"name\":\"rl-4u\",\"description\":\"rl-probe\"}"
      }
    },
    "update/5/fields": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes-4\",\"_rev\":\"<rev>\",\"name\":\"rl-4v\"}"
      }
    },
    "patch/4": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes-4\",\"_rev\":\"<rev>\",\"name\":\"rl-4v\",\"description\":\"rl-5\"}"
      }
    },
    "query/2/filter": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"keys\":[\"pagedResultsCookie\",\"result\",\"resultCount\",\"totalPagedResults\",\"totalPagedResultsPolicy\"],\"ids\":[\"rl-probe-openidm-writes\",\"rl-probe-openidm-writes-4\"],\"recordKeys\":[\"_id,_rev,description,name\",\"_id,_rev,description,name,rlNotInSchema\"],\"pagedResultsCookie\":null,\"totalPagedResultsPolicy\":\"NONE\",\"totalPagedResults\":-1,\"resultCount\":2}"
      }
    },
    "query/3/fields": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"keys\":[\"pagedResultsCookie\",\"result\",\"resultCount\",\"totalPagedResults\",\"totalPagedResultsPolicy\"],\"ids\":[\"rl-probe-openidm-writes\",\"rl-probe-openidm-writes-4\"],\"recordKeys\":[\"_id,_rev,name\",\"_id,_rev,name\"],\"pagedResultsCookie\":null,\"totalPagedResultsPolicy\":\"NONE\",\"totalPagedResults\":-1,\"resultCount\":2}"
      }
    },
    "query/2/none": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"keys\":[\"pagedResultsCookie\",\"result\",\"resultCount\",\"totalPagedResults\",\"totalPagedResultsPolicy\"],\"ids\":[],\"recordKeys\":[],\"pagedResultsCookie\":null,\"totalPagedResultsPolicy\":\"NONE\",\"totalPagedResults\":-1,\"resultCount\":0}"
      }
    },
    "query/2/bad-filter": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: The value 'name eq' for parameter '_queryFilter' could not be parsed as a valid query filter"
    },
    "query/2/no-filter": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: You must use exactly one of [_queryId, _queryExpression, _queryFilter]."
    },
    "query/2/absent-type": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Resource &#39;managed/rlNoSuchType&#39; not found"
    },
    "query/2/queryId": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"keys\":[\"pagedResultsCookie\",\"result\",\"resultCount\",\"totalPagedResults\",\"totalPagedResultsPolicy\"],\"countIsNumber\":true,\"mine\":[\"_id,_rev\",\"_id,_rev\"]}"
      }
    },
    "query/2/queryExpression": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Query Expressions are not supported when using DS as the repo"
    },
    "query/2/two-kinds": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: You must use exactly one of [_queryId, _queryExpression, _queryFilter]."
    },
    "patch/wrong-rev": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: The resource could not be accessed because the expected version &#39;0&#39; does not match the current version &#39;<uuid>&#39;"
    },
    "delete/wrong-rev": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Assertion Failed: Entry uid&#61;rl-probe-openidm-writes-4,ou&#61;role,o&#61;alpha,o&#61;root,ou&#61;identities cannot be removed because the request contained an LDAP assertion control and the associated filter did not match the contents of the entry"
    },
    "delete/4/fields": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes-4\",\"_rev\":\"<rev>\",\"name\":\"rl-4v\"}"
      }
    },
    "action/4/validateObject": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"result\": true, \"failedPolicyRequirements\": [  ] }",
        "json": "{\"failedPolicyRequirements\":[],\"result\":true}"
      }
    },
    "action/3/validateObject": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"result\": true, \"failedPolicyRequirements\": [  ] }",
        "json": "{\"failedPolicyRequirements\":[],\"result\":true}"
      }
    },
    "action/5/validateObject": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "{ \"result\": true, \"failedPolicyRequirements\": [  ] }",
        "json": "{\"failedPolicyRequirements\":[],\"result\":true}"
      }
    },
    "action/2/unknown": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Expecting String containing one of: patch triggerSyncCheck updateLastSync"
    },
    "delete/3": {
      "ok": true,
      "value": {
        "type": "object",
        "string": "[object Object]",
        "json": "{\"_id\":\"rl-probe-openidm-writes\",\"_rev\":\"<rev>\",\"name\":\"rl-a2\",\"description\":\"rl-8\",\"rlNotInSchema\":\"x\"}"
      }
    },
    "delete": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: No Such Entry: The search base entry &#39;uid&#61;rl-probe-openidm-writes,ou&#61;role,o&#61;alpha,o&#61;root,ou&#61;identities&#39; does not exist"
    },
    "delete/absent": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: No Such Entry: The search base entry &#39;uid&#61;rl-probe-openidm-writes,ou&#61;role,o&#61;alpha,o&#61;root,ou&#61;identities&#39; does not exist"
    },
    "update/absent": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: No Such Entry: The search base entry &#39;uid&#61;rl-probe-openidm-writes,ou&#61;role,o&#61;alpha,o&#61;root,ou&#61;identities&#39; does not exist"
    }
  }, {
    given: {
      "managed": {
        "managed/alpha_role": []
      }
    },
    allowUndeclared: {"openidmWrites":true},
    gap: {
      reason:
        "AIC reports write failures as JavaException wrappers of ResourceExceptionScriptAdapter carrying the LDAP DN; the mock throws a rhino-local Error with AIC's message phrase instead, rather than forge a Java exception. validateObject answers from tenant policy, which the mock does not model (it returns {}). Writes were not captured live, so the write channel is not judged. openidm.query: a filter the local parser does not support is reported as unmocked rather than as AIC's parse error, and an unseeded managed type throws the harness's missing-fixture error rather than AIC's \"Resource … not found\".",
      differs: ["cleanup/before","create/duplicate","update/wrong-rev","patch/bad-operation","query/2/bad-filter","query/2/no-filter","query/2/absent-type","query/2/queryExpression","query/2/two-kinds","patch/wrong-rev","delete/wrong-rev","action/4/validateObject","action/3/validateObject","action/5/validateObject","action/2/unknown","delete","delete/absent","update/absent"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-utils-interop", "fixtures/binding-utils-interop.script.js", {
    "ecdsa/verify/p1363": {
      "ok": true,
      "value": true
    },
    "ecdsa/verify/der": {
      "ok": true,
      "value": false
    },
    "ecdsa/verify/p1363/tampered": {
      "ok": true,
      "value": false
    },
    "rsa/generated/encoding": {
      "ok": true,
      "value": {
        "publicKey": {
          "length": 294,
          "head": "30820122300d06092a864886f70d01010105000382010f00"
        },
        "privateKey": "020100300d06092a864886f70d0101010500"
      }
    },
    "ecdsa/generated/encoding": {
      "ok": true,
      "value": {
        "publicKey": {
          "length": 91,
          "head": "3059301306072a8648ce3d020106082a8648ce3d03010703420004"
        },
        "privateKey": {
          "length": 67,
          "head": "3041020100301306072a8648ce3d020106082a8648ce3d030107042730250201010420"
        }
      }
    },
    "digest/js-array": {
      "ok": true,
      "value": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    },
    "digest/SHA-1": {
      "ok": true,
      "value": "a9993e364706816aba3e25717850c26c9cd0d89d"
    },
    "digest/SHA-512": {
      "ok": true,
      "value": "ddaf35a193617aba"
    },
    "digest/MD5": {
      "ok": false,
      "error": "InternalError: Algorithm must be one of [SHA-256, SHA-384, SHA-1, SHA-512]"
    },
    "digest/object": {
      "ok": false,
      "error": "InternalError: Algorithm must be one of [SHA-256, SHA-384, SHA-1, SHA-512]"
    },
    "aes/js-array-key": {
      "ok": true,
      "value": "1d25821c3e311eea2d4dd8633a25c1b5"
    },
    "aes/js-array-data": {
      "ok": true,
      "value": "1d25821c3e311eea2d4dd8633a25c1b5"
    },
    "aes/key-24": {
      "ok": true,
      "value": "377f35478afd40126bdcb4d5f9be9ee6"
    },
    "aes/key-5": {
      "ok": false,
      "error": "JavaException: org.forgerock.openam.scripting.bindings.crypto.ScriptCryptoException: java.security.InvalidKeyException: Invalid AES key length: 5 bytes"
    },
    "hmac/SHA-512": {
      "ok": true,
      "value": "3926a207c8c42b0c"
    },
    "types/bytesToString/js-array": {
      "ok": true,
      "value": "ab"
    },
    "types/stringToBytes/number": {
      "ok": true,
      "value": "3132"
    },
    "base64/encode/number": {
      "ok": true,
      "value": "MTI="
    },
    "crypto/randomUUID/1": {
      "ok": false,
      "error": "InternalError: Can't find method org.forgerock.openam.scripting.bindings.crypto.ScriptCryptoService.randomUUID(number). (AIC Rhino Let Probe#128)"
    },
    "crypto/getRandomValues/js-object": {
      "ok": false,
      "error": "InternalError: Can't find method org.forgerock.openam.scripting.bindings.crypto.ScriptCryptoService.getRandomValues(object). (AIC Rhino Let Probe#132)"
    }
  }, {
    gap: {
      reason:
        "ECDSA interop (P1363 accepted, DER false), generated key encodings, digest/AES/HMAC edges and JS-array handling match. Still different: a 5-byte AES key fails on both, but AIC wraps it as JavaException ScriptCryptoException; randomUUID(1) and getRandomValues({}) fail on both, but with the harness arity message and without AIC's (name#line) suffix respectively.",
      differs: ["aes/key-5","crypto/randomUUID/1","crypto/getRandomValues/js-object"],
    },
  }),
  // Live payload, verbatim (probe run 2026-09-29).
  ng("binding-callbacks-getters", "fixtures/binding-callbacks-getters.script.js", {
    "getStringAttributeInputCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[value]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "string",
        "string": "value",
        "array": false,
        "length": 5
      }
    },
    "getChoiceCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[[I@<hash>]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "object",
        "string": "[I@<hash>",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1
      }
    },
    "getNameCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[Ada]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "string",
        "string": "Ada",
        "array": false,
        "length": 3
      }
    },
    "getPasswordCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "string",
        "string": "",
        "array": false,
        "length": 0
      }
    },
    "getHiddenValueCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "{ \"hidden\": \"hidden\" }",
        "array": false,
        "keys": [
          "hidden"
        ],
        "size": 1
      },
      "first": {
        "type": "object",
        "string": "null",
        "array": false
      }
    },
    "getTextInputCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "string",
        "string": "",
        "array": false,
        "length": 0
      }
    },
    "getNumberAttributeInputCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[7.0]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "number",
        "string": "7",
        "array": false
      }
    },
    "getBooleanAttributeInputCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[true]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "boolean",
        "string": "true",
        "array": false
      }
    },
    "getConfirmationCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[0]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "number",
        "string": "0",
        "array": false
      }
    },
    "getLanguageCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[en_US]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "string",
        "string": "en_US",
        "array": false,
        "length": 5
      }
    },
    "getIdpCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[{nodeName=IdPCallback, redirectUri=https://example.com, request=, acrValues=[], userInfo=null, clientId=client, requestNativeAppForUserInfo=false, requestUri=, nonce=nonce, token=, provider=provider, scope=[openid], tokenType=}]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "object",
        "string": "{ \"nodeName\": \"IdPCallback\", \"redirectUri\": \"https://example.com\", \"request\": \"\", \"acrValues\": [  ], \"userInfo\": null, \"clientId\": \"client\", \"requestNativeAppForUserInfo\": false, \"requestUri\": \"\", \"nonce\": \"nonce\", \"token\": \"\", \"provider\": \"provider\", \"scope\": [ \"openid\" ], \"tokenType\": \"\" }",
        "array": false,
        "keys": [
          "acrValues",
          "clientId",
          "nodeName",
          "nonce",
          "provider",
          "redirectUri",
          "request",
          "requestNativeAppForUserInfo",
          "requestUri",
          "scope",
          "token",
          "tokenType",
          "userInfo"
        ],
        "size": 13
      }
    },
    "getValidatedPasswordCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[{validateOnly=false, value=}]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "object",
        "string": "{ \"validateOnly\": false, \"value\": \"\" }",
        "array": false,
        "keys": [
          "validateOnly",
          "value"
        ],
        "size": 2
      }
    },
    "getValidatedUsernameCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[{validateOnly=false, value=}]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "object",
        "string": "{ \"validateOnly\": false, \"value\": \"\" }",
        "array": false,
        "keys": [
          "validateOnly",
          "value"
        ],
        "size": 2
      }
    },
    "getHttpCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[]",
        "array": false,
        "keys": [],
        "length": 0,
        "size": 0
      },
      "first": null
    },
    "getX509CertificateCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[]",
        "array": false,
        "keys": [],
        "length": 0,
        "size": 0
      },
      "first": null
    },
    "getConsentMappingCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[false]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "boolean",
        "string": "false",
        "array": false
      }
    },
    "getDeviceProfileCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "string",
        "string": "",
        "array": false,
        "length": 0
      }
    },
    "getKbaCreateCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[{selectedAnswer=, selectedQuestion=}]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "object",
        "string": "{ \"selectedAnswer\": \"\", \"selectedQuestion\": \"\" }",
        "array": false,
        "keys": [
          "selectedAnswer",
          "selectedQuestion"
        ],
        "size": 2
      }
    },
    "getSelectIdPCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "string",
        "string": "",
        "array": false,
        "length": 0
      }
    },
    "getTermsAndConditionsCallbacks": {
      "ok": true,
      "list": {
        "type": "object",
        "string": "[false]",
        "array": false,
        "keys": [
          "0"
        ],
        "length": 1,
        "size": 1
      },
      "first": {
        "type": "boolean",
        "string": "false",
        "array": false
      }
    },
    "isEmpty": {
      "ok": true,
      "value": false
    }
  }, {
    given: {
      "callbacks": [
        {
          "type": "StringAttributeInputCallback",
          "value": "value",
          "validateOnly": false
        },
        {
          "type": "ChoiceCallback",
          "value": 0
        },
        {
          "type": "NameCallback",
          "value": "Ada"
        },
        {
          "type": "PasswordCallback",
          "value": ""
        },
        {
          "type": "HiddenValueCallback",
          "value": "hidden",
          "id": "hidden"
        },
        {
          "type": "TextInputCallback",
          "value": ""
        },
        {
          "type": "NumberAttributeInputCallback",
          "value": 7,
          "validateOnly": false
        },
        {
          "type": "BooleanAttributeInputCallback",
          "value": true,
          "validateOnly": false
        },
        {
          "type": "ConfirmationCallback",
          "value": 0
        },
        {
          "type": "LanguageCallback",
          "value": "en_US"
        },
        {
          "type": "IdPCallback",
          "provider": "provider",
          "clientId": "client",
          "redirectUri": "https://example.com",
          "scopes": [
            "openid"
          ],
          "nonce": "nonce",
          "acrValues": [],
          "request": "",
          "acceptsJSON": false,
          "requestUri": "",
          "token": "",
          "tokenType": ""
        },
        {
          "type": "ValidatedCreatePasswordCallback",
          "value": "",
          "validateOnly": false
        },
        {
          "type": "ValidatedCreateUsernameCallback",
          "value": "",
          "validateOnly": false
        },
        {
          "type": "TextOutputCallback"
        },
        {
          "type": "DeviceProfileCallback",
          "value": ""
        },
        {
          "type": "KbaCreateCallback",
          "selectedQuestion": "",
          "selectedAnswer": ""
        },
        {
          "type": "SelectIdPCallback",
          "value": ""
        },
        {
          "type": "TermsAndConditionsCallback",
          "value": false
        }
      ]
    },
    gap: {
      reason:
        "The two remaining differences are getChoiceCallbacks (AIC returns int[], which cannot be constructed through AM’s class shutter) and getConsentMappingCallbacks (visit one emitted a TextOutputCallback after consentMappingCallback failed, so no consent callback body can seed its observed [false] value).",
      differs: ["getChoiceCallbacks","getConsentMappingCallbacks"],
    },
  }),
  // Live callbacks, parsed from the probe run's /authenticate body (2026-09-29).
  realCase({
    name: "binding-callbacks-builder",
    kind: "nextgen",
    origin: "scripts/rhino-script-tester/fixtures/binding-callbacks-builder.script.js",
    given: firstVisit(),
    expect: {
      // AIC sends the callbacks and decides nothing; the harness records the
      // script's outcome alongside them, as for every probe case here.
      outcome: "ok",
      callbacks: [
        {
          "type": "ChoiceCallback",
          "prompt": "probe-0",
          "choices": [
            "left-1",
            "right-1"
          ],
          "defaultChoice": 1,
          "radio": true
        },
        {
          "type": "SuspendedTextOutputCallback",
          "message": "probe-11",
          "messageType": "2"
        },
        {
          "type": "TextInputCallback",
          "prompt": "probe-20",
          "defaultText": "probe-21"
        },
        {
          "type": "TextInputCallback",
          "prompt": "probe-30",
          "defaultText": ""
        },
        {
          "type": "TextOutputCallback",
          "message": "probe-40",
          "messageType": "4"
        },
        {
          "type": "MetadataCallback",
          "data": {
            "marker": "object-50"
          }
        },
        {
          "type": "StringAttributeInputCallback",
          "name": "probe-60",
          "prompt": "probe-61",
          "required": false,
          "policies": [],
          "failedPolicies": [
            "left-64",
            "right-64"
          ],
          "validateOnly": false,
          "value": "probe-62"
        },
        {
          "type": "StringAttributeInputCallback",
          "name": "probe-70",
          "prompt": "probe-71",
          "required": false,
          "policies": {
            "marker": "object-74"
          },
          "failedPolicies": [],
          "validateOnly": false,
          "value": "probe-72"
        },
        {
          "type": "StringAttributeInputCallback",
          "name": "probe-80",
          "prompt": "probe-81",
          "required": false,
          "policies": [],
          "failedPolicies": [],
          "validateOnly": false,
          "value": "probe-82"
        },
        {
          "type": "StringAttributeInputCallback",
          "name": "probe-90",
          "prompt": "probe-91",
          "required": false,
          "policies": {
            "marker": "object-94"
          },
          "failedPolicies": [
            "left-96",
            "right-96"
          ],
          "validateOnly": false,
          "value": "probe-92"
        },
        {
          "type": "NumberAttributeInputCallback",
          "name": "probe-100",
          "prompt": "probe-101",
          "required": false,
          "policies": {
            "marker": "object-104"
          },
          "failedPolicies": [],
          "validateOnly": false,
          "value": 103
        },
        {
          "type": "NumberAttributeInputCallback",
          "name": "probe-110",
          "prompt": "probe-111",
          "required": false,
          "policies": [],
          "failedPolicies": [
            "left-114",
            "right-114"
          ],
          "validateOnly": false,
          "value": 113
        },
        {
          "type": "NumberAttributeInputCallback",
          "name": "probe-120",
          "prompt": "probe-121",
          "required": false,
          "policies": [],
          "failedPolicies": [],
          "validateOnly": false,
          "value": 123
        },
        {
          "type": "NumberAttributeInputCallback",
          "name": "probe-130",
          "prompt": "probe-131",
          "required": false,
          "policies": {
            "marker": "object-134"
          },
          "failedPolicies": [
            "left-136",
            "right-136"
          ],
          "validateOnly": false,
          "value": 133
        },
        {
          "type": "BooleanAttributeInputCallback",
          "name": "probe-140",
          "prompt": "probe-141",
          "required": false,
          "policies": [],
          "failedPolicies": [
            "left-144",
            "right-144"
          ],
          "validateOnly": false,
          "value": true
        },
        {
          "type": "BooleanAttributeInputCallback",
          "name": "probe-150",
          "prompt": "probe-151",
          "required": false,
          "policies": {
            "marker": "object-154"
          },
          "failedPolicies": [
            "left-156",
            "right-156"
          ],
          "validateOnly": false,
          "value": true
        },
        {
          "type": "BooleanAttributeInputCallback",
          "name": "probe-160",
          "prompt": "probe-161",
          "required": false,
          "policies": [],
          "failedPolicies": [],
          "validateOnly": false,
          "value": true
        },
        {
          "type": "BooleanAttributeInputCallback",
          "name": "probe-170",
          "prompt": "probe-171",
          "required": false,
          "policies": {
            "marker": "object-174"
          },
          "failedPolicies": [],
          "validateOnly": false,
          "value": true
        },
        {
          "type": "LanguageCallback"
        },
        {
          "type": "IdPCallback",
          "provider": "probe-190",
          "clientId": "probe-191",
          "redirectUri": "probe-192",
          "scopes": [
            "left-193",
            "right-193"
          ],
          "nonce": "probe-194",
          "acrValues": [
            "left-197",
            "right-197"
          ],
          "request": "probe-195",
          "acceptsJSON": true,
          "requestUri": "probe-196"
        },
        {
          "type": "IdPCallback",
          "provider": "probe-200",
          "clientId": "probe-201",
          "redirectUri": "probe-202",
          "scopes": [
            "left-203",
            "right-203"
          ],
          "nonce": "probe-204",
          "acrValues": [
            "left-207",
            "right-207"
          ],
          "request": "probe-205",
          "acceptsJSON": true,
          "requestUri": "probe-206"
        },
        {
          "type": "ConsentMappingCallback",
          "name": "probe-260",
          "displayName": "probe-261",
          "icon": "probe-262",
          "accessLevel": "probe-263",
          "isRequired": true,
          "message": "probe-265",
          "fields": [
            "left-264",
            "right-264"
          ]
        },
        {
          "type": "ConsentMappingCallback",
          "name": "probe-270",
          "displayName": "probe-272",
          "icon": "probe-273",
          "accessLevel": "probe-274",
          "isRequired": true,
          "message": "probe-271",
          "fields": [
            null
          ]
        },
        {
          "type": "DeviceProfileCallback",
          "metadata": true,
          "location": false,
          "message": "probe-282"
        },
        {
          "type": "KbaCreateCallback",
          "prompt": "probe-290",
          "predefinedQuestions": [
            "left-291",
            "right-291"
          ],
          "allowUserDefinedQuestions": true
        },
        {
          "type": "SelectIdPCallback",
          "providers": {
            "marker": "object-300"
          },
          "value": ""
        },
        {
          "type": "TermsAndConditionsCallback",
          "version": "probe-310",
          "terms": "probe-311",
          "createDate": "probe-312"
        },
        {
          "type": "ChoiceCallback",
          "prompt": "probe-320",
          "choices": [
            "left-321",
            "right-321"
          ],
          "defaultChoice": 1
        },
        {
          "type": "PasswordCallback",
          "prompt": "probe-330"
        },
        {
          "type": "NameCallback",
          "prompt": "probe-340"
        },
        {
          "type": "NameCallback",
          "prompt": "probe-350"
        },
        {
          "type": "HiddenValueCallback",
          "value": "probe-361",
          "id": "probe-360"
        },
        {
          "type": "RedirectCallback",
          "redirectUrl": "probe-370",
          "redirectMethod": "probe-372",
          "trackingCookie": false,
          "redirectData": {
            "marker": "object-371"
          }
        },
        {
          "type": "RedirectCallback",
          "redirectUrl": "probe-380",
          "redirectMethod": "probe-382",
          "trackingCookie": false,
          "redirectData": {
            "marker": "object-381"
          }
        },
        {
          "type": "RedirectCallback",
          "redirectUrl": "probe-390",
          "redirectMethod": "probe-392",
          "trackingCookie": false,
          "redirectData": {
            "marker": "object-391"
          }
        },
        {
          "type": "RedirectCallback",
          "redirectUrl": "probe-400",
          "redirectMethod": "probe-402",
          "trackingCookie": false,
          "redirectData": {
            "marker": "object-401"
          }
        },
        {
          "type": "RedirectCallback",
          "redirectUrl": "probe-410",
          "redirectMethod": "probe-412",
          "trackingCookie": true,
          "redirectData": {
            "marker": "object-411"
          }
        },
        {
          "type": "RedirectCallback",
          "redirectUrl": "probe-420",
          "redirectMethod": "probe-422",
          "trackingCookie": true,
          "redirectData": {
            "marker": "object-421"
          }
        },
        {
          "type": "ConfirmationCallback",
          "prompt": "",
          "messageType": 1,
          "options": [],
          "optionType": 0,
          "defaultOption": 1
        },
        {
          "type": "ConfirmationCallback",
          "prompt": "",
          "messageType": 2,
          "options": [
            "left-421",
            "right-421"
          ],
          "optionType": -1,
          "defaultOption": 1
        },
        {
          "type": "ConfirmationCallback",
          "prompt": "probe-430",
          "messageType": 0,
          "options": [],
          "optionType": 2,
          "defaultOption": 3
        },
        {
          "type": "ConfirmationCallback",
          "prompt": "probe-440",
          "messageType": 1,
          "options": [
            "left-442",
            "right-442"
          ],
          "optionType": -1,
          "defaultOption": 0
        },
        {
          "type": "PollingWaitCallback",
          "waitTime": "probe-450",
          "message": "probe-451"
        },
        {
          "type": "TextOutputCallback",
          "message": "probe-461",
          "messageType": "1"
        },
        {
          "type": "ValidatedCreateUsernameCallback",
          "policies": {
            "marker": "object-471"
          },
          "failedPolicies": [
            "left-473",
            "right-473"
          ],
          "validateOnly": true,
          "prompt": "probe-470"
        },
        {
          "type": "ValidatedCreateUsernameCallback",
          "policies": {
            "marker": "object-481"
          },
          "failedPolicies": [],
          "validateOnly": true,
          "prompt": "probe-480"
        },
        {
          "type": "ValidatedCreatePasswordCallback",
          "echoOn": false,
          "policies": {
            "marker": "object-492"
          },
          "failedPolicies": [
            "left-494",
            "right-494"
          ],
          "validateOnly": false,
          "prompt": "probe-490"
        },
        {
          "type": "ValidatedCreatePasswordCallback",
          "echoOn": false,
          "policies": {
            "marker": "object-502"
          },
          "failedPolicies": [],
          "validateOnly": false,
          "prompt": "probe-500"
        }
      ],
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
