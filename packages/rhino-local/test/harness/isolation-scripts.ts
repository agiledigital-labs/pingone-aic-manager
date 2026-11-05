/**
 * The polluter and probe scripts for the isolation tests: one in-process
 * (isolation.e2e.test.ts), one across real Vitest files
 * (pool-files.e2e.test.ts, via test/fixtures/jvm-pool/).
 */
export const LIBRARIES = {
  // Module state, a regex literal, and a function that reads a global through
  // the library's own scope chain.
  stateful: [
    "var counter = 0;",
    "var re = /a/g;",
    "exports.bump = function () { counter += 1; return counter; };",
    "exports.re = re;",
    "exports.arrayLeak = function () { return [].leakArray; };",
  ].join("\n"),
  // A tagged template: Rhino caches its call-site array inside the compiled script.
  tagged: "function tag(strings) { return strings; }\nexports.site = function () { return tag`a${1}b`; };",
};

/** Everything a script can reach, vandalised. Non-enumerable so the case's own harvest survives. */
export const POLLUTER = `
leakGlobal = 1;
Object.defineProperty(Array.prototype, "leakArray", { value: 1, configurable: true });
Object.defineProperty(Object.prototype, "leakObject", { value: 1, configurable: true });
String.prototype.trim = function () { return "poisoned"; };
var lib = require("stateful");
lib.bump();
lib.bump();
lib.re.lastIndex = 7;
lib.re.leak = 1;
require("tagged").site();
nodeState.leak = 1;
callbacksBuilder.leak = 1;
utils.crypto.randomUUID = function () { return "poisoned"; };
logger.error("polluter log");
nodeState.putShared("polluted", true);
JSON.leak = 1;
// Replaced, not broken: the case's own harvest still has to parse.
JSON.parse = (function (parse) { var f = function () { return parse.apply(JSON, arguments); }; f.poisoned = true; return f; })(JSON.parse);
action.goTo("done");
`;

export const PROBE = `
var lib = require("stateful");
var probe = {
  leakGlobal: typeof leakGlobal,
  arrayProto: [].leakArray === undefined,
  objectProto: ({}).leakObject === undefined,
  trim: " a ".trim(),
  json: JSON.leak === undefined && JSON.parse.poisoned === undefined,
  nodeState: nodeState.leak === undefined,
  callbacksBuilder: callbacksBuilder.leak === undefined,
  uuid: utils.crypto.randomUUID() !== "poisoned",
  moduleState: lib.bump(),
  regexLastIndex: lib.re.lastIndex,
  regexProps: lib.re.leak === undefined,
  libraryGlobals: lib.arrayLeak() === undefined,
  taggedSite: require("tagged").site().leakArray === undefined,
};
nodeState.putShared("probe", JSON.stringify(probe));
action.goTo("done");
`;

export const FRESH = {
  leakGlobal: "undefined",
  arrayProto: true,
  objectProto: true,
  trim: "a",
  json: true,
  nodeState: true,
  callbacksBuilder: true,
  uuid: true,
  moduleState: 1,
  regexLastIndex: 0,
  regexProps: true,
  libraryGlobals: true,
  taggedSite: true,
};

