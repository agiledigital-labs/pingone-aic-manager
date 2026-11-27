/** Journey-specific overlay over the existing AM binding runtime. Host work uses the runner bridge. */
import { readFileSync } from "node:fs";
import { casePreamble, type CasePreamble } from "../bindings/preamble.ts";
import { bindingsJsonPath, generatedJsPath } from "../paths.ts";
import type { Given, CallbackEffect } from "../case/types.ts";
import type { Document } from "../idm/types.ts";

export type ScriptState = {
  shared: Document;
  transient: Document;
  callbacks: CallbackEffect[];
  tree: string;
  inner: boolean;
  identityResource: string;
  realm: string;
  scriptName: string;
  scriptId: string;
  existingSession?: Record<string, string>;
  /** Properties earlier nodes of this journey put on the session being built. */
  priorSessionProperties?: Record<string, string>;
  headers?: Record<string, string[]>;
  cookies?: Record<string, string>;
  parameters?: Record<string, string[]>;
  /** Buckets whose objectAttributes AM hands a script as a Java map rather than a JS object. */
  javaMaps?: { shared: boolean; transient: boolean };
  /** LIBRARY script sources by name: what `require(name)` loads, as on the tenant. */
  libraries?: Record<string, string>;
  /** The environment's ESVs, as `systemEnv.getProperty` reads them. */
  esv?: Record<string, string | null>;
  /**
   * A custom (node designer) node's script: it sees only the bindings doc 25 measured there
   * (`properties`, `nodeState.get` / `putShared` and the assigned `outcome`); reaching any other
   * one is missing behaviour naming it.
   */
  customNode?: boolean;
};

/*
 * Runs once, before author code, while the runner's bridge global exists. Every
 * function it installs comes from the bridge (Java), so author code cannot reach
 * this closure: no installed function's __parent__ leads back here, and the
 * bridge holds the harvest, the error message and the host transport in Java
 * fields. See java/AmHostBridge.java for what that does and does not cover.
 */
/** Every global binding a generated stub belongs to, read from the stubs themselves. */
function stubRoots(): string[] {
  const generated = readFileSync(generatedJsPath, "utf8");
  const roots = new Set<string>();
  for (const match of generated.matchAll(/__rhinoLocalNotMocked\("([^".]+)/g))
    if (match[1]) roots.add(match[1]);
  return [...roots].sort();
}
/** utils members docs/api/12 measured; any other utils member is refused. */
const MEASURED_UTILS = [
  ...["base64", "base64url"].flatMap((codec) =>
    ["encode", "decode", "decodeToBytes", "btoa", "atob"].map((name) => `${codec}.${name}`),
  ),
  "types.stringToBytes",
  "types.bytesToString",
  "crypto.randomUUID",
  "crypto.getRandomValues",
  "crypto.checkBcrypt",
  ...["encrypt", "decrypt", "generateKey", "digest", "sign", "verify", "deriveKey"].map(
    (name) => `crypto.subtle.${name}`,
  ),
];
/**
 * The names a custom node's script must not reach: every next-gen decision binding but
 * `nodeState`, read from the binding list itself, plus the globals the harness adds beside them.
 */
let blockedCache: string[] | undefined;
export function customBlocked(): string[] {
  if (blockedCache) return blockedCache;
  const list = JSON.parse(readFileSync(bindingsJsonPath, "utf8")) as {
    bindings: { name: string }[];
  };
  const names = new Set(list.bindings.map((binding) => binding.name));
  for (const name of ["existingSession", "require", "sharedState", "transientState"])
    names.add(name);
  names.delete("nodeState");
  blockedCache = [...names].sort();
  return blockedCache;
}
/** What a custom node's `nodeState` offers; doc 25 measured these two methods only. */
const CUSTOM_NODE_STATE = ["get", "putShared"];
/**
 * A custom node's restricted `nodeState`, built in the decision scope where the bridge is reachable
 * and handed to the custom node's own scope (java/CustomNodeScope.java). Every other method the
 * next-gen `nodeState` has is a refusal naming it. The legacy `getShared` and `getTransient` are
 * not among them -- the decision overlay deletes them first, as doc 25 measured -- so calling one
 * is an ordinary TypeError, a throw like any other.
 */
const customNodeState = `
(function (host, allowed) {
  var state = nodeState, view = {};
  Object.keys(state).forEach(function (key) {
    if (typeof state[key] === 'function' && allowed.indexOf(key) === -1)
      view[key] = host('refusal', 'custom node binding: nodeState.' + key);
  });
  view.get = function (key) { return state.get(key); };
  view.putShared = function (key, value) { state.putShared(key, value); return view; };
  __rhinoLocalCustomNodeState = view;
})(__rhinoLocalJourneyHost, ${JSON.stringify(CUSTOM_NODE_STATE)});
`;
const overlay = (roots: string[], maps: { shared: boolean; transient: boolean }) => `
(function (host, roots, measuredUtils, maps) {
  host('install', __rhinoLocal, action);
  // Script logger lines go to the bridge, which holds them in Java: they survive a
  // throw, which no harvest does, and the author cannot rebind them.
  __rhinoLocal.logs = { push: host('log') };
  // Every rhino-local diagnostic goes through this global; in a journey it is
  // missing behaviour, decided in Java by which function ran, not by error text.
  __rhinoLocalFail = host('unsupported');
  ['shared', 'transient'].forEach(function (bucket) {
    var value = __rhinoLocal[bucket].objectAttributes;
    if (maps[bucket] && value !== null && typeof value === 'object' && !Array.isArray(value))
      __rhinoLocal[bucket].objectAttributes = __rhinoLocalAsJavaMap(value);
  });

  // Generated stubs reach a global hook author code could rebind; replace each
  // with a Java refusal named after its binding path.
  var stub = /__rhinoLocalNotMocked\\("([^"]*)", "([^"]*)"/;
  var seen = [];
  function unstub(object, depth) {
    if (depth > 3 || seen.indexOf(object) !== -1) return;
    seen.push(object);
    Object.keys(object).forEach(function (key) {
      var value = object[key];
      if (typeof value === 'function') {
        var match = stub.exec(String(value));
        if (match) object[key] = host('refusal', match[1] + '.' + match[2]);
      } else if (value !== null && typeof value === 'object' &&
          Object.prototype.toString.call(value) === '[object Object]') {
        unstub(value, depth + 1);
      }
    });
  }
  var global = this;
  roots.forEach(function (name) {
    var binding = global[name];
    if (binding !== null && typeof binding === 'object') unstub(binding, 0);
  });
  __rhinoLocalNotMocked = host('notMocked');

  function refuse(object, prefix, allowed) {
    Object.keys(object).forEach(function (key) {
      if (typeof object[key] === 'function' && allowed.indexOf(key) === -1) {
        object[key] = host('refusal', prefix + '.' + key);
      }
    });
  }
  refuse(action, 'action', ['goTo', 'withErrorMessage', 'withLockoutMessage', 'putSessionProperty',
    'removeSessionProperty', 'toString']);
  action.withErrorMessage = host('errorMessage');
  action.withLockoutMessage = host('chain');
  // The measured next-gen factory does not expose the legacy bucket getters.
  delete nodeState.getShared;
  delete nodeState.getTransient;
  refuse(nodeState, 'nodeState', ['get', 'getObject', 'putShared', 'putTransient',
    'mergeShared', 'mergeTransient', 'isDefined', 'remove', 'keys']);
  var hostMethods = ['read', 'create', 'update', 'patch', 'delete', 'query', 'action'];
  hostMethods.forEach(function (method) {
    openidm[method] = host('method', method);
  });
  refuse(openidm, 'openidm', hostMethods);
  httpClient.send = host('http');
  // utils is nested: walk every own member, enumerable or not. Measured members
  // run locally and record provenance; anything else is refused.
  (function observe(object, prefix, depth) {
    if (depth > 4) return;
    Object.getOwnPropertyNames(object).forEach(function (key) {
      var value = object[key], path = prefix + key;
      if (typeof value === 'function') {
        object[key] = measuredUtils.indexOf(path) !== -1
          ? host('observe', value, 'utils.' + path)
          : host('refusal', 'utils.' + path);
      } else if (value !== null && typeof value === 'object') {
        observe(value, path + '.', depth + 1);
      }
    });
  })(utils, '', 0);
  [['idRepository', idRepository], ['secrets', secrets],
    ['cacheManager', cacheManager]].forEach(function (pair) {
    refuse(pair[1], pair[0], []);
  });
})(__rhinoLocalJourneyHost, ${JSON.stringify(roots)}, ${JSON.stringify(MEASURED_UTILS)},
  ${JSON.stringify(maps)});
`;

/** The case preamble plus the journey overlay; only the overlay is new per step. */
export function journeyPreamble(state: ScriptState): CasePreamble {
  const given: Given = {
    engine: "next-gen",
    sharedState: state.shared,
    transientState: state.transient,
    callbacks: state.callbacks,
    realm: state.realm,
    scriptName: state.scriptName,
    loggerScriptId: state.scriptId,
    requestHeaders: state.headers ?? {},
    requestCookies: state.cookies ?? {},
    requestParameters: state.parameters ?? {},
    // An ESV the environment does not define reads as null (docs/api/12, library section).
    esv: state.esv ?? {},
    esvUndeclared: "absent",
    ...(state.libraries && Object.keys(state.libraries).length
      ? { libraries: state.libraries }
      : {}),
    ...(state.existingSession ? { existingSession: state.existingSession } : {}),
    ...(state.priorSessionProperties ? { priorSessionProperties: state.priorSessionProperties } : {}),
    bindings: {
      journey: {
        name: state.tree,
        innerJourney: state.inner,
        identityResource: state.identityResource,
        mustRun: false,
      },
    },
  };
  const base = casePreamble(given);
  return {
    ...base,
    preamble:
      base.preamble +
      overlay(stubRoots(), state.javaMaps ?? { shared: false, transient: false }) +
      (state.customNode ? customNodeState : ""),
  };
}
