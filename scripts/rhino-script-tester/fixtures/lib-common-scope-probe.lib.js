// Library body for the "common" scope-injection probe. Uploaded as a LIBRARY
// script named rhino-lib-common-scope-probe and required by
// lib-common-scope-consumer.script.js. Asks four things a `common.run(this, …)`
// library depends on:
//   1. can the library read the node's bindings through the `this` it is given?
//   2. does a property it writes onto that object resolve as a bare identifier
//      in the caller's arrow function (option A)?
//   3. does module-level state survive between journey invocations (module
//      caching)?
//   4. what JS/Java types do state, header, parameter and cookie values arrive
//      as?
// Safe to delete.
var moduleCalls = 0;

function describe(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  var d = {
    typeof: typeof v,
    tag: Object.prototype.toString.call(v),
  };
  try {
    d.javaClass = v.getClass ? String(v.getClass().getName()) : null;
  } catch (e) {
    d.javaClass = "err:" + String(e);
  }
  try {
    d.isArray = Array.isArray(v);
  } catch (e) {
    d.isArray = "err";
  }
  try {
    d.length = v.length === undefined ? null : Number(v.length);
  } catch (e) {
    d.length = "err";
  }
  try {
    d.json = JSON.stringify(v);
  } catch (e) {
    d.json = "err:" + String(e);
  }
  try {
    d.str = String(v);
  } catch (e) {
    d.str = "err:" + String(e);
  }
  return d;
}

function attempt(fn) {
  try {
    return { ok: true, value: fn() };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
}

exports.hit = function () {
  moduleCalls = moduleCalls + 1;
  return moduleCalls;
};

exports.readBindings = function (scope) {
  return {
    typeofScope: typeof scope,
    hasNodeState: attempt(function () {
      return typeof scope.nodeState;
    }),
    hasRequestHeaders: attempt(function () {
      return typeof scope.requestHeaders;
    }),
    hasRequestParameters: attempt(function () {
      return typeof scope.requestParameters;
    }),
    hasRequestCookies: attempt(function () {
      return typeof scope.requestCookies;
    }),
    hasExistingSession: attempt(function () {
      return typeof scope.existingSession;
    }),
    hasAction: attempt(function () {
      return typeof scope.action;
    }),
    hasLogger: attempt(function () {
      return typeof scope.logger;
    }),
    stateString: attempt(function () {
      return describe(scope.nodeState.get("myValue1"));
    }),
    stateObject: attempt(function () {
      return describe(scope.nodeState.get("myValue2"));
    }),
    stateObjectField: attempt(function () {
      return describe(scope.nodeState.get("myValue2").get("region"));
    }),
    stateObjectDot: attempt(function () {
      return describe(scope.nodeState.get("myValue2").region);
    }),
    stateMissing: attempt(function () {
      return describe(scope.nodeState.get("aicProbeAbsent"));
    }),
    header: attempt(function () {
      return describe(scope.requestHeaders.get("x-aic-probe"));
    }),
    headerFirst: attempt(function () {
      return describe(scope.requestHeaders.get("x-aic-probe")[0]);
    }),
    param: attempt(function () {
      return describe(scope.requestParameters.get("probeq"));
    }),
    cookie: attempt(function () {
      return describe(scope.requestCookies.get("aicprobe"));
    }),
    cookieDot: attempt(function () {
      return describe(scope.requestCookies.aicprobe);
    }),
  };
};

// Option A: write each value onto the caller's scope object.
exports.inject = function (scope, values) {
  var report = {};
  Object.keys(values).forEach(function (k) {
    report[k] = attempt(function () {
      scope[k] = values[k];
      return scope[k] === values[k];
    });
  });
  return report;
};

// Option A, the shape the real library would use: inject, then run the body.
exports.run = function (scope, values, body) {
  exports.inject(scope, values);
  return body();
};

// Option B: hand the values to the body as an argument.
exports.runWith = function (values, body) {
  return body(values);
};
