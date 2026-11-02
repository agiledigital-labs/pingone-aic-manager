/** Journey-specific overlay over the existing AM binding runtime. Host work uses the runner bridge. */
import { readFileSync } from "node:fs";
import { mockPreamble } from "../bindings/preamble.ts";
import { generatedJsPath } from "../paths.ts";
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
  headers?: Record<string, string[]>;
  cookies?: Record<string, string>;
  parameters?: Record<string, string[]>;
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
const overlay = (roots: string[]) => `
(function (host, roots) {
  host('install', __rhinoLocalHarvest, __rhinoLocal, action);

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
  refuse(action, 'action', ['goTo', 'withErrorMessage', 'withLockoutMessage', 'toString']);
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
  [['idRepository', idRepository], ['utils', utils], ['secrets', secrets],
    ['cacheManager', cacheManager], ['systemEnv', systemEnv]].forEach(function (pair) {
    refuse(pair[1], pair[0], []);
  });
  require = host('refusal', 'require library resolution');

  // Runtime mock errors denote unsupported emulation, not catchable tenant failures.
  function protect(object, prefix) {
    Object.keys(object).forEach(function (key) {
      if (typeof object[key] === 'function') {
        object[key] = host('guard', object, object[key], prefix + '.' + key);
      }
    });
  }
  protect(action, 'action');
  protect(journey, 'journey');
  protect(nodeState, 'nodeState');
  protect(callbacks, 'callbacks');
  protect(callbacksBuilder, 'callbacksBuilder');
})(__rhinoLocalJourneyHost, ${JSON.stringify(roots)});
`;

export function journeyPreamble(state: ScriptState): string {
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
    ...(state.existingSession ? { existingSession: state.existingSession } : {}),
    bindings: {
      journey: {
        name: state.tree,
        innerJourney: state.inner,
        identityResource: state.identityResource,
        mustRun: false,
      },
    },
  };
  return mockPreamble(given) + overlay(stubRoots());
}
