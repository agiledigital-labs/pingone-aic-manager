/** Journey-specific overlay over the existing AM binding runtime. Host work uses the runner bridge. */
import { mockPreamble } from "../bindings/preamble.ts";
import type { Given, CallbackEffect } from "../case/types.ts";
import type { Document } from "../idm/types.ts";
export type ScriptState = { shared: Document; transient: Document; callbacks: CallbackEffect[]; tree: string; inner: boolean; identityResource: string; realm: string; scriptName: string; scriptId: string; existingSession?: Record<string, string>; headers?: Record<string, string[]>; cookies?: Record<string, string>; parameters?: Record<string, string[]> };
export function journeyPreamble(state: ScriptState): string {
  const given: Given = { engine: "next-gen", sharedState: state.shared, transientState: state.transient, callbacks: state.callbacks,
    realm: state.realm, scriptName: state.scriptName, loggerScriptId: state.scriptId,
    requestHeaders: state.headers ?? {}, requestCookies: state.cookies ?? {}, requestParameters: state.parameters ?? {},
    ...(state.existingSession ? { existingSession: state.existingSession } : {}),
    bindings: { journey: { name: state.tree, innerJourney: state.inner, identityResource: state.identityResource, mustRun: false } } };
  return mockPreamble(given) + `
var __journeyErrorMessage = null;
(function(host) {
  function absent(name) { return host('missing', name); }
  function invoke(method, args) {
    var result = JSON.parse(String(host(method, JSON.stringify(args, function(key, value) {
      if (typeof value === 'function') return absent('AM host function arguments');
      if (typeof value === 'number' && (!isFinite(value) || (value === 0 && 1/value === -Infinity))) return absent('AM host non-finite or negative-zero arguments');
      return value;
    }))));
    if (result.error) {
      var e = new Error(result.error.message); e.name = 'JavaException';
      e.code = result.error.code; e.detail = result.error.detail; throw e;
    }
    return result.result;
  }
  __rhinoLocalNotMocked = function(binding, method) { return absent(binding + '.' + method); };
  function refuse(object, prefix, allowed) {
    Object.keys(object).forEach(function(key) {
      if (typeof object[key] === 'function' && allowed.indexOf(key) === -1) {
        object[key] = function() { return absent(prefix + '.' + key); };
      }
    });
  }
  refuse(action, 'action', ['goTo', 'withErrorMessage', 'withLockoutMessage', 'toString']);
  action.withErrorMessage = function(message) { __journeyErrorMessage = String(message); return action; };
  action.withLockoutMessage = function(message) { return action; };
  // The measured next-gen factory does not expose the legacy bucket getters.
  delete nodeState.getShared; delete nodeState.getTransient;
  refuse(nodeState, 'nodeState', ['get','getObject','putShared','putTransient','mergeShared','mergeTransient','isDefined','remove','keys']);
  ['read','create','update','patch','delete','query','action'].forEach(function(method) {
    openidm[method] = function() { return invoke(method, Array.prototype.slice.call(arguments)); };
  });
  refuse(openidm, 'openidm', ['read','create','update','patch','delete','query','action']);
  httpClient.send = function(uri, options) {
    var reply = invoke('http', [String(uri), options || {}]);
    var response = { status: reply.status, ok: reply.status >= 200 && reply.status < 300,
      headers: reply.headers || {}, statusText: String(reply.status),
      json: function() { return reply.body; },
      text: function() { return typeof reply.body === 'string' ? reply.body : JSON.stringify(reply.body); } };
    return {get: function() {return response;}};
  };
  [ ['idRepository',idRepository], ['utils',utils], ['secrets',secrets], ['cacheManager',cacheManager], ['systemEnv',systemEnv] ].forEach(function(pair) { refuse(pair[1], pair[0], []); });
  require = function() { return absent('require library resolution'); };
  // Runtime mock errors denote unsupported emulation, not catchable tenant failures.
  function protect(object, prefix) {
    Object.keys(object).forEach(function(key) {
      var original = object[key];
      if (typeof original !== 'function') return;
      object[key] = function() {
        try {return original.apply(object, arguments);}
        catch(e) {if (String(e).indexOf('rhino-local:') !== -1) return absent(prefix + '.' + key + ': ' + String(e)); throw e;}
      };
    });
  }
  protect(action, 'action'); protect(journey, 'journey'); protect(nodeState, 'nodeState'); protect(callbacks, 'callbacks'); protect(callbacksBuilder, 'callbacksBuilder');
})(__rhinoLocalJourneyHost);
`;
}
export const JOURNEY_HARVEST = `\n;JSON.stringify({effects:JSON.parse(__rhinoLocalHarvest()),errorMessage:__journeyErrorMessage});\n`;
