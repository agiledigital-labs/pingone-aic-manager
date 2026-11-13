// Handwritten overlay on generated/scripted-decision-mocks.cjs.
// Evaluated inside AM's Rhino 1.7.14 (VERSION_DEFAULT): no let, no top-level
// const, no for...of, no object shorthand, no destructuring, no default
// parameters, and none of Map / Set / Symbol / Promise / WeakMap / WeakSet.
//
// Methods this file does not replace keep throwing
// `rhino-local: not mocked: <binding>.<method> …`.

var __rhinoLocal = {
  shared: {},
  transient: {},
  secure: {},
  initialShared: {},
  initialTransient: {},
  initialSecure: {},
  sessionProperties: {},
  initialSessionProperties: {},
  managed: {},
  esv: {},
  esvProvided: false,
  esvUndeclared: "error",
  secrets: {},
  secretsProvided: false,
  bindings: {},
  submittedCallbacks: null,
  httpStubs: [],
  openidmActions: [],
  openidmFailures: [],
  managedWriteFaults: null,
  managedPolicy: null,
  managedInvalidDefault: null,
  openidmCallCounts: {},
  generatedId: 0,
  outcome: null,
  loggerScriptId: "<unseeded-script-id>",
  callbacks: [],
  openidm: [],
  http: [],
  logs: [],
  identityWrites: [],
  faults: [],
  identityAttributes: {},
  identityCustomAttrs: {},
  identityCustomAttrsOwnedKeys: {},
  identityGenerations: {},
  libraries: {},
  requireCache: {},
};

var sharedState;
var transientState;
var systemEnv;
var require;

function __rhinoLocalClone(value) {
  return JSON.parse(JSON.stringify(value));
}

// A record field is data whatever its name: a plain assignment to `__proto__`
// would set the prototype instead of storing the field (astra-r1 P2-2, r2 P2).
function __rhinoLocalSetOwn(target, key, value) {
  Object.defineProperty(target, key, { value: value, enumerable: true, writable: true, configurable: true });
}

// docs/api/12 (2026-11-04), as measured on the wire: an `undefined` field of
// openidm.create content is sent as null (only the top level was measured, so
// nested undefined still drops), and an httpClient.send body sends undefined as
// null at any depth, the body itself included.
function __rhinoLocalCloneUndefinedAsNull(value, topLevelOnly) {
  if (topLevelOnly) {
    if (value === null || typeof value !== "object") {
      return __rhinoLocalClone(value);
    }
    return JSON.parse(
      JSON.stringify(value, function (key, field) {
        return field === undefined && this === value ? null : field;
      })
    );
  }
  return JSON.parse(
    JSON.stringify(value, function (key, field) {
      return field === undefined ? null : field;
    })
  );
}

// The httpClient.send body is recorded as a snapshot taken at send time (this
// harness's choice, so later mutation cannot change the record), converted as
// AIC converts an object body. Measured (docs/api/12 §2, §3, §5, 2026-11-04):
//   undefined -> null at any depth;
//   NaN, 1/0, -1/0 -> the strings "NaN", "Infinity", "-Infinity";
//   -0 -> -0.0 on the wire (kept as -0 here; the harvest's JSON records 0, the
//     same lost precision as 1.0 recorded as 1);
//   new java.util.Date(0) -> 0. Other instants are recorded as epoch
//     milliseconds, which is assumed: at 0 every unit agrees;
//   a root self-cycle (b.self = b) -> a catchable InternalError, nothing sent.
//     Other cycles of plain objects are assumed to do the same, since the
//     message names only [object Object];
//   a JS Date, RegExp or function -> a 1-1.7 KB dump of the Rhino object. That
//     is not reproducible here and no script means to send it, so it is a
//     harness fault: thrown, and failing the run even if the script catches it
//     (AIC would not have thrown, so the script's catch is not AIC's path).
// toJSON is never called (AIC did not apply Date.prototype.toJSON; other toJSON
// methods are assumed the same). An Error is unmeasured, but it is a built-in
// like Date and RegExp, so AIC most likely dumps it too: a fault, since any
// recorded guess could pass a body assertion AIC would fail. Values the
// measurement did not cover (other Java objects, other built-ins) pass through
// JSON as before, unmeasured.
function __rhinoLocalSendBody(value, path, ancestors) {
  var kind;
  var out;
  var item;
  var key;
  var i;
  if (value === undefined) {
    return null;
  }
  if (typeof value === "number") {
    if (value !== value) {
      return "NaN";
    }
    if (value === Infinity || value === -Infinity) {
      return value > 0 ? "Infinity" : "-Infinity";
    }
    return value;
  }
  if (value === null || typeof value !== "object" && typeof value !== "function") {
    return value;
  }
  kind = Object.prototype.toString.call(value);
  // The script-defect/ family: AIC's behaviour is measured (it sends a dump
  // and carries on), and faulting is a deliberate refusal to pass a script
  // that would misbehave there, not a gap in the emulation.
  if (kind === "[object Date]") {
    throw __rhinoLocalFault("script-defect/send-body-js-date", 
      "rhino-local: httpClient.send " + path + " is a JS Date: AIC sends a ~1 KB dump of the Rhino NativeDate object without the time value, not the date (docs/api/12 §5); send date.toISOString() or date.getTime()"
    );
  }
  if (kind === "[object RegExp]") {
    throw __rhinoLocalFault("script-defect/send-body-regexp", 
      "rhino-local: httpClient.send " + path + " is a RegExp: AIC sends a ~1 KB dump of the Rhino NativeRegExp object without its source or flags (docs/api/12 §5); send regexp.source as a string"
    );
  }
  if (kind === "[object Error]") {
    throw __rhinoLocalFault("send-body-error", 
      "rhino-local: httpClient.send " + path + " is an Error: what AIC sends for one is unmeasured (it most likely dumps the Rhino object, as it does a JS Date or RegExp, docs/api/12 §5); send e.message or String(e)"
    );
  }
  if (typeof value === "function") {
    throw __rhinoLocalFault("script-defect/send-body-function", 
      "rhino-local: httpClient.send " + path + " is a function: AIC sends a ~1.7 KB dump of the Rhino function object, which exposes the script's name, its encoded source and the name of every binding and variable in the script (docs/api/12 §5); leave it out of the body"
    );
  }
  if (kind === "[object JavaObject]" && __rhinoLocalIsJavaDate(value)) {
    return Number(value.getTime());
  }
  if (kind !== "[object Object]" && kind !== "[object Array]") {
    item = JSON.stringify(value);
    return item === undefined ? undefined : JSON.parse(item);
  }
  for (i = 0; i < ancestors.length; i += 1) {
    if (ancestors[i] === value) {
      // Measured with plain objects only; a cycle through an array may report
      // another message (or none), so that shape is a fault.
      if (path.indexOf("[") === -1) {
        throw new InternalError("Cannot produce JSON from [object Object]");
      }
      throw __rhinoLocalFault("send-body-array-cycle", 
        "rhino-local: httpClient.send " + path + " refers back to an enclosing value through an array (a cycle): AIC's error for this shape is unmeasured"
      );
    }
  }
  ancestors.push(value);
  if (kind === "[object Array]") {
    out = [];
    for (i = 0; i < value.length; i += 1) {
      item = __rhinoLocalSendBody(value[i], path + "[" + i + "]", ancestors);
      out.push(item === undefined ? null : item);
    }
  } else {
    out = {};
    for (key in value) {
      if (Object.prototype.hasOwnProperty.call(value, key)) {
        item = __rhinoLocalSendBody(value[key], path + "." + key, ancestors);
        if (item !== undefined) {
          out[key] = item;
        }
      }
    }
  }
  ancestors.pop();
  return out;
}

function __rhinoLocalIsJavaDate(value) {
  try {
    return value instanceof java.util.Date;
  } catch (e) {
    // java.util.Date hidden by the class shutter: the script cannot hold one.
    return false;
  }
}

// A harness fault: behaviour the harness does not emulate, which the script
// must not be able to swallow. `id` is a stable slug (docs/rhino-local-gaps.md
// lists them); never derive one from a line number or the message text.
//
// Recorded with the runner's per-job recorder (java/HarnessFaults.java), which
// the runner reports whatever the job's outcome, so the run fails even when the
// script catches this and then succeeds, or throws something of its own. With
// no recorder (a journey, or a host without the Java runner) it is kept for the
// harvest, and a journey's __rhinoLocalFail is itself uncatchable and takes the
// id. Thrown as usual either way, so the script sees what it would see first.
// The runner's recorder, held where the script cannot reach it: setting a
// property of __rhinoLocal must not silence every later fault. Captured as the
// first act of __rhinoLocalSeed, so a fault raised while seeding is recorded.
var __rhinoLocalFaultSink = (function () {
  var sink = null;
  var channel = {
    capture: function () {
      sink =
        typeof __rhinoLocalHarnessFault === "function" ? __rhinoLocalHarnessFault : null;
    },
    record: function (id, message) {
      if (sink === null) {
        return false;
      }
      sink(id, message);
      return true;
    },
  };
  return typeof Object.freeze === "function" ? Object.freeze(channel) : channel;
})();

function __rhinoLocalFault(id, message) {
  if (!__rhinoLocalFaultSink.record(id, message)) {
    __rhinoLocal.faults.push(id + ": " + message.replace(/^rhino-local: /, ""));
  }
  return __rhinoLocalFail(message, id);
}

// A refusal whose remedy is a fixture the test did not supply, not missing
// emulation. Still a fault: a script that catches it and goes on passes
// locally on a path AIC would not take.
function __rhinoLocalFixtureFault(id, message) {
  return __rhinoLocalFault("fixture/" + id, message);
}

// What a Java `toArray()` hands a script: indexable with a working `length`,
// but not a JS array (measured: Array.isArray false). `length` is hidden so
// for...in and Object.keys see only indices, as they would on an array. A
// real Java array is not built: whether element identity and method access
// survive Rhino's conversion into Object[] is unproven here.
// A Java array as Rhino surfaces it: indexed, with length, not
// Array.isArray, yet Array.prototype methods resolve — getAttributeValues(n)
// .toArray().map(String) ran on AIC (live-identity-store-families, 2026-10-01).
function __rhinoLocalArrayLike(items) {
  var out = typeof Object.create === "function" ? Object.create(Array.prototype) : {};
  var i;
  for (i = 0; i < items.length; i += 1) {
    out[i] = items[i];
  }
  __rhinoLocalHide(out, "length", items.length);
  return out;
}

// Every "rhino-local:" diagnostic is built here: an approximation the local
// runtime cannot stand behind. The script tester throws it as an Error; the
// journey emulator replaces this global with a host function that fails the
// run as missing behaviour, which no script catch can intercept.
function __rhinoLocalFail(message) {
  return new Error(message);
}

// An error AIC itself raises, which the harness emulates and the script may
// rightly catch. Kept apart from __rhinoLocalFail, which a journey replaces
// with its uncatchable missing-behaviour channel: an emulated error must stay
// catchable in every lane.
function __rhinoLocalEmulated(message) {
  return new Error(message);
}

function __rhinoLocalHide(object, name, fn) {
  if (typeof Object.defineProperty === "function") {
    Object.defineProperty(object, name, {
      enumerable: false,
      configurable: true,
      writable: true,
      value: fn,
    });
    return;
  }
  object[name] = fn;
}

// A JavaException as AIC's Rhino surfaces one. `name` is not an own
// enumerable key: AIC's `Object.keys(e)` is `fileName,lineNumber` (second
// probe, 2026-11-06, docs/api/12), which the JVM lane's Error supplies and
// node:vm's has none of.
function __rhinoLocalJavaException(message) {
  var error = new Error(message);
  __rhinoLocalHide(error, "name", "JavaException");
  return error;
}

// CREST's reason phrase for each status: the table src/idm/errors.ts uses
// (test/bindings/effects.test.ts pins the two equal).
var __rhinoLocalCrestReasons = {
  400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found",
  405: "Method Not Allowed", 409: "Conflict", 412: "Precondition Failed",
  500: "Internal Server Error", 501: "Not Implemented", 503: "Service Unavailable",
};

// The ResourceExceptionScriptAdapter AM wraps an IDM failure in, for a
// failure whose status is known. `e.javaException` is the adapter (measured
// on the illegal-assertion 400, docs/api/12): it answers getCode() and
// getReason(), and its string is the message; nothing else is exposed
// (getMessage and getClass are TypeErrors on AIC). A status with no reason
// in CREST's table faults on getReason() rather than invent one.
// `detail`, when known, is `{ value }`: what getDetail() wraps. Unknown, the
// detail is a fault on call, never an invented null.
function __rhinoLocalAdapterException(text, code, detail) {
  var error = __rhinoLocalJavaException(
    "org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: " + text
  );
  var adapter = {};
  __rhinoLocalHide(adapter, "getCode", function () { return code; });
  __rhinoLocalHide(adapter, "getReason", function () {
    if (!Object.prototype.hasOwnProperty.call(__rhinoLocalCrestReasons, code)) {
      throw __rhinoLocalFault("openidm-failure-reason",
        "rhino-local: javaException.getReason(): CREST's reason for status " + code + " is not in the emulator's table");
    }
    return __rhinoLocalCrestReasons[code];
  });
  __rhinoLocalHide(adapter, "getDetail", function () {
    if (arguments.length !== 0) {
      throw __rhinoLocalFault("openidm-failure-detail",
        "rhino-local: javaException.getDetail(" + __rhinoLocalScriptSignature(arguments) + ") is unmeasured (only getDetail() was probed)");
    }
    if (detail === undefined) {
      throw __rhinoLocalFault("openidm-failure-detail",
        "rhino-local: javaException.getDetail(): this failure's detail is unknown; give the openidmFailures stub a reply.detail (null for a 404 or 400)");
    }
    return __rhinoLocalDetailValue(detail.value, []);
  });
  __rhinoLocalHide(adapter, "toString", function () { return error.message; });
  __rhinoLocalHide(error, "javaException", adapter);
  return error;
}

// What AM's adapter getDetail() returns: a CREST org.forgerock.json.JsonValue,
// measured 2026-11-08 (docs/api/12, openidm-error-detail probe) for a policy
// 403 (an object) and a 404 and a 400 (null). Measured: get(key) (a JsonValue
// again), isNull(), getObject() (the plain value), String() (CREST's one-line
// form; the same JsonValue.toString the IDM lane measured, with its limits),
// JSON.stringify (that string, quoted), and Object.keys/for...in (the
// member list below, in that order).
//
// Measured 2026-11-10 (openidm-detail-asstring probe): get(i) by index,
// asString(), and presence. asString() is the JS string for a string value
// and null for a null one (a null detail, a missing key, an index past the
// end); on a boolean, object or array it throws AIC's catchable
// "<pointer>: Expecting a java.lang.String but got <class>", where the
// pointer starts at /detail. `typeof d.<member>` is "function" and
// `<member> in d` is true for every method below, so a script's feature
// detection answers as on AIC; only CALLING an unmeasured member (or using
// it as a value beyond typeof, `in` and comparison) faults. `d.class` is the
// class shutter's refusal.
var __rhinoLocalDetailMembers = ("getClass,equalTo,setObject,put,asList,required,patch,number,putPermissive,iterator," +
  "putIfNotNull,isNumber,merge,asString,defined,isNotNull,add,getObject,pointer,spliterator,notNull,asMapOfList," +
  "asAsync,diff,collection,list,isMap,asInteger,asLong,contains,as,size,null,asDouble,asBoolean,object,wait,string," +
  "keys,asMap,notifyAll,isCollection,isList,notify,remove,isBoolean,stream,addIfNotNull,hashCode,get,isString,copy," +
  "class,map,asCollection,forEach,asNumber,clear,defaultTo,getPointer,expect,boolean,addPermissive,isDefined," +
  "equals,isNull,clone,toString,isEqualTo").split(",");

function __rhinoLocalDetailUnmeasured(what) {
  return __rhinoLocalFault("openidm-failure-detail", "rhino-local: javaException.getDetail()" + what + " is unmeasured");
}

// CREST's JsonPointer for an error message: /detail, then each key or index.
// A segment JSON Pointer would escape (`/`, `~`) was never seen, so it faults.
function __rhinoLocalDetailPointer(path) {
  var i;
  var out = "/detail";
  for (i = 0; i < path.length; i += 1) {
    if (/[\/~]/.test(String(path[i]))) {
      throw __rhinoLocalDetailUnmeasured(": a pointer segment needing an escape (" + JSON.stringify(String(path[i])) + ")");
    }
    out += "/" + path[i];
  }
  return out;
}

// The Java class CREST names for a value in "Expecting a java.lang.String but
// got …" (measured for these three; a number's class was not).
function __rhinoLocalDetailClass(value) {
  if (typeof value === "boolean") {
    return "java.lang.Boolean";
  }
  if (Array.isArray(value)) {
    return "java.util.ArrayList";
  }
  if (value !== null && typeof value === "object") {
    return "java.util.LinkedHashMap";
  }
  return null;
}

function __rhinoLocalDetailString(value) {
  var i;
  var keys;
  var parts = [];
  if (value === null || typeof value === "boolean") {
    return String(value);
  }
  if (typeof value === "number") {
    if (Math.floor(value) !== value || Math.abs(value) > 1e15) {
      throw __rhinoLocalDetailUnmeasured(": the string form of a non-integral number");
    }
    return String(value);
  }
  if (typeof value === "string") {
    if (/[^\x20-\x7e]|["\\]/.test(value)) {
      throw __rhinoLocalDetailUnmeasured(": the string form of a string needing an escape");
    }
    return '"' + value + '"';
  }
  if (Array.isArray(value)) {
    if (value.length === 0) {
      throw __rhinoLocalDetailUnmeasured(": the string form of an empty array");
    }
    for (i = 0; i < value.length; i += 1) {
      parts.push(__rhinoLocalDetailString(value[i]));
    }
    return "[ " + parts.join(", ") + " ]";
  }
  keys = Object.keys(value);
  if (keys.length === 0) {
    throw __rhinoLocalDetailUnmeasured(": the string form of an empty object");
  }
  for (i = 0; i < keys.length; i += 1) {
    parts.push(JSON.stringify(keys[i]) + ": " + __rhinoLocalDetailString(value[keys[i]]));
  }
  return "{ " + parts.join(", ") + " }";
}

// The probe called get with one string and the other three with none; any
// other call shape is unmeasured, so it faults before JavaScript's ignored-
// argument and string-coercion rules can answer for it.
function __rhinoLocalDetailNoArgs(name, args) {
  if (args.length !== 0) {
    throw __rhinoLocalDetailUnmeasured("." + name + "(" + __rhinoLocalScriptSignature(args) + ")");
  }
}

function __rhinoLocalDetailValue(value, path) {
  var jv = {};
  var child = function (key, next) {
    return __rhinoLocalDetailValue(next, path.concat([key]));
  };
  var asString = function () {
    if (arguments.length === 1 && typeof arguments[0] === "number") {
      throw __rhinoLocalInternalError("Can't find method org.forgerock.json.JsonValue.asString(number).");
    }
    __rhinoLocalDetailNoArgs("asString", arguments);
    if (value === null || typeof value === "string") {
      return value;
    }
    var cls = __rhinoLocalDetailClass(value);
    if (cls === null) {
      throw __rhinoLocalDetailUnmeasured(".asString() on a " + typeof value);
    }
    throw __rhinoLocalInternalError(__rhinoLocalDetailPointer(path) + ": Expecting a java.lang.String but got " + cls);
  };
  var asStringText = "function asString() {/*\njava.lang.String asString()\n*/}\n";
  __rhinoLocalHide(asString, "toString", function () {
    return asStringText;
  });
  // Rendered by any route (its own toString, a borrowed Function.prototype.
  // toString, String(), concatenation), asString is the measured text.
  __rhinoLocalBrandMember(asString, "asString", false, asStringText);
  var measured = {
    get: function (key) {
      if (arguments.length !== 1 || (typeof key !== "string" && typeof key !== "number")) {
        throw __rhinoLocalDetailUnmeasured(".get(" + __rhinoLocalScriptSignature(arguments) + ")");
      }
      // Only integral indices within a Java int were measured; how Rhino
      // converts a fraction, an infinity or a wider number to get(int) was
      // not. Checked before every value shortcut, the null one included.
      if (typeof key === "number" && !(isFinite(key) && Math.floor(key) === key && key >= -2147483648 && key <= 2147483647)) {
        throw __rhinoLocalDetailUnmeasured(".get(" + key + ")");
      }
      if (value === null) {
        return child(key, null);
      }
      if (typeof key === "string") {
        if (Array.isArray(value)) {
          return child(key, null);
        }
        if (typeof value !== "object") {
          throw __rhinoLocalDetailUnmeasured(".get(string) on a " + typeof value + " value");
        }
        return child(key, Object.prototype.hasOwnProperty.call(value, key) ? value[key] : null);
      }
      if (Array.isArray(value)) {
        if (key < 0) {
          throw __rhinoLocalInternalError(__rhinoLocalDetailPointer(path) + ": List index out of range: " + key);
        }
        return child(key, key < value.length ? value[key] : null);
      }
      if (typeof value === "object" || typeof value === "string") {
        return child(key, null);
      }
      throw __rhinoLocalDetailUnmeasured(".get(number) on a " + typeof value + " value");
    },
    isNull: function () {
      __rhinoLocalDetailNoArgs("isNull", arguments);
      return value === null;
    },
    getObject: function () {
      __rhinoLocalDetailNoArgs("getObject", arguments);
      return __rhinoLocalClone(value);
    },
    toString: function () {
      __rhinoLocalDetailNoArgs("toString", arguments);
      return __rhinoLocalDetailString(value);
    },
    asString: asString,
  };
  var i;
  for (i = 0; i < __rhinoLocalDetailMembers.length; i += 1) {
    (function (name) {
      if (Object.prototype.hasOwnProperty.call(measured, name)) {
        if (name !== "asString") {
          // Measured to call, not to render: any string form of it faults.
          __rhinoLocalBrandMember(measured[name], name, false, null);
        }
        Object.defineProperty(jv, name, { enumerable: true, configurable: true, writable: true, value: measured[name] });
      } else if (name === "class") {
        Object.defineProperty(jv, name, {
          enumerable: true,
          configurable: true,
          get: function () {
            throw __rhinoLocalShutterError("java.lang.Class");
          },
        });
      } else {
        Object.defineProperty(jv, name, { enumerable: true, configurable: true, writable: true, value: __rhinoLocalDetailStub(name) });
      }
    })(__rhinoLocalDetailMembers[i]);
  }
  __rhinoLocalHide(jv, "toJSON", function () {
    return __rhinoLocalDetailString(value);
  });
  return jv;
}

// An unmeasured JsonValue method: present, so typeof and `in` answer as on
// AIC (measured), but calling it, rendering it as a string, binding it, or
// serialising it, faults. Serialisation is refused at the JSON.stringify
// boundary (__rhinoLocalJsonValue), which sees every value native is about to
// write, whichever toJSON or replacer produced it; the stub's own toJSON
// covers the property-list path, where native asks it directly.
var __rhinoLocalStubKey = "__rhinoLocalStub";

// Brands a detail member function with its {stub, name, unmeasured, text}
// row. `unmeasured` marks a stub (every use but typeof/in faults); `text` is
// the member's measured string form, or null when rendering it is
// unmeasured. The Function.prototype.bind and toString replacements and the
// JSON.stringify boundary decide per receiver from this row.
function __rhinoLocalBrandMember(fn, name, unmeasured, text) {
  __rhinoLocalHide(fn, __rhinoLocalStubKey, { stub: fn, name: name, unmeasured: unmeasured, text: text });
}

// A branded detail member's row, or null. Read as the list row is: an own data
// property whose row points back at the value, so a copy or inheritor of the
// key does not count.
function __rhinoLocalStubRow(value) {
  if (typeof value !== "function") {
    return null;
  }
  var own;
  try {
    own = Object.getOwnPropertyDescriptor(value, __rhinoLocalStubKey);
  } catch (e) {
    return null;
  }
  var row = own !== undefined && "value" in own ? own.value : null;
  return row !== null && typeof row === "object" && row.stub === value ? row : null;
}

function __rhinoLocalDetailStub(name) {
  var stub = function () {
    throw __rhinoLocalDetailUnmeasured("." + name + "(" + __rhinoLocalScriptSignature(arguments) + ")");
  };
  __rhinoLocalBrandMember(stub, name, true, null);
  // A bound copy would carry none of the guards; call and apply would invoke
  // the stub anyway, and refuse here with the same fault for a clearer name.
  ["bind", "call", "apply"].forEach(function (method) {
    __rhinoLocalHide(stub, method, function () {
      throw __rhinoLocalDetailUnmeasured("." + name + "." + method + "(…)");
    });
  });
  __rhinoLocalHide(stub, "toString", function () {
    throw __rhinoLocalDetailUnmeasured("." + name + " as a string");
  });
  // JSON.stringify skips a function without calling toString, but it does ask
  // any object, functions included, for toJSON: so serialising the stub,
  // bare or nested, faults too instead of writing undefined, {} or [null].
  __rhinoLocalHide(stub, "toJSON", function () {
    throw __rhinoLocalDetailUnmeasured("." + name + " under JSON.stringify");
  });
  return stub;
}

// Where each method's accepted argument count comes from (E19, decision B).
// On AIC every binding below is a Java object, and Rhino resolves a Java
// method by argument count: a call matching no overload throws a catchable
//   InternalError: Can't find method <declaring class>.<method>(<arg types>).
// measured for session.get() (docs/api/12, existingSession table) and
// utils.crypto.randomUUID(1) (corpus binding-utils-interop, live payload), and
// seen for three more methods (docs/api/12: getMemberships(), setAct(object);
// corpus getRandomValues(object)).
//   measured -- that refusal was observed for this method.
//   assumed  -- the local range is the method's overload set in AIC's own
//               binding introspection (docs/api/bindings/scripted-decision-
//               next.json, named by `java`), and the refusal is assumed to
//               take the measured shape.
// A path missing here has no such basis: its range is the harness's guess, and
// a call outside it is a harness fault (`arity-unknown/<path>`) -- AIC may
// accept the call, so a script catching the local throw would pass on a path
// AIC never takes.
var __rhinoLocalArityBasis = (function () {
  var basis = {};
  var pkg = "org.forgerock.openam.";
  function add(java, provenance, paths) {
    var i;
    for (i = 0; i < paths.length; i += 1) {
      basis[paths[i]] = { java: pkg + java, provenance: provenance };
    }
  }
  add("scripting.javascript.MapScriptWrapper", "measured", ["existingSession.get"]);
  add("scripting.bindings.crypto.ScriptCryptoService", "measured", ["utils.crypto.randomUUID"]);
  add("scripting.bindings.crypto.ScriptCryptoService", "assumed", ["utils.crypto.getRandomValues"]);
  add("auth.node.api.NodeStateScriptWrapper", "assumed", [
    "nodeState.get", "nodeState.isDefined", "nodeState.putShared", "nodeState.putTransient",
    "nodeState.remove", "nodeState.keys", "nodeState.getObject", "nodeState.mergeTransient",
  ]);
  // mergeShared() with no argument: AIC's `Can't find method ...mergeShared().`
  // (E19 live probe, 2026-11-06), which also measured the class it names.
  add("auth.node.api.NodeStateScriptWrapper", "measured", ["nodeState.mergeShared"]);
  add("auth.nodes.script.ActionWrapper", "assumed", [
    "action.goTo", "action.withErrorMessage", "action.withHeader", "action.withIdentifiedUser",
    "action.withIdentifiedAgent", "action.withStage", "action.withDescription",
    "action.withLockoutMessage", "action.putSessionProperty", "action.removeSessionProperty",
    "action.withMaxSessionTime", "action.withMaxIdleTime", "action.suspend",
  ]);
  add("auth.nodes.script.ScriptedCallbacksBuilder", "assumed", [
    "callbacksBuilder.nameCallback", "callbacksBuilder.passwordCallback",
    "callbacksBuilder.textOutputCallback", "callbacksBuilder.hiddenValueCallback",
    "callbacksBuilder.confirmationCallback", "callbacksBuilder.choiceCallback",
    "callbacksBuilder.radioChoiceCallback", "callbacksBuilder.suspendedTextOutputCallback",
    "callbacksBuilder.textInputCallback", "callbacksBuilder.scriptTextOutputCallback",
    "callbacksBuilder.languageCallback", "callbacksBuilder.idPCallback",
    "callbacksBuilder.httpCallback", "callbacksBuilder.x509CertificateCallback",
    "callbacksBuilder.consentMappingCallback", "callbacksBuilder.deviceProfileCallback",
    "callbacksBuilder.kbaCreateCallback", "callbacksBuilder.selectIdPCallback",
    "callbacksBuilder.termsAndConditionsCallback", "callbacksBuilder.metadataCallback",
    "callbacksBuilder.pollingWaitCallback", "callbacksBuilder.redirectCallback",
    "callbacksBuilder.stringAttributeInputCallback", "callbacksBuilder.numberAttributeInputCallback",
    "callbacksBuilder.booleanAttributeInputCallback", "callbacksBuilder.validatedUsernameCallback",
    "callbacksBuilder.validatedPasswordCallback",
  ]);
  add("auth.nodes.script.ScriptedCallbacksWrapper", "assumed", [
    "callbacks.isEmpty", "callbacks.getNameCallbacks", "callbacks.getPasswordCallbacks",
    "callbacks.getHiddenValueCallbacks", "callbacks.getDeviceProfileCallbacks",
    "callbacks.getKbaCreateCallbacks", "callbacks.getSelectIdPCallbacks",
    "callbacks.getTermsAndConditionsCallbacks", "callbacks.getTextInputCallbacks",
    "callbacks.getStringAttributeInputCallbacks", "callbacks.getNumberAttributeInputCallbacks",
    "callbacks.getBooleanAttributeInputCallbacks", "callbacks.getConfirmationCallbacks",
    "callbacks.getLanguageCallbacks", "callbacks.getIdpCallbacks",
    "callbacks.getValidatedPasswordCallbacks", "callbacks.getValidatedUsernameCallbacks",
    "callbacks.getHttpCallbacks", "callbacks.getX509CertificateCallbacks",
    "callbacks.getConsentMappingCallbacks", "callbacks.getChoiceCallbacks",
  ]);
  add("scripting.api.identity.ScriptedIdentityRepositoryScriptWrapper", "assumed", [
    "idRepository.getIdentity",
  ]);
  add("scripting.api.secrets.ScriptedSecretsFacadeImpl", "assumed", [
    "secrets.getGenericSecret", "secrets.getDecryptionKey", "secrets.getEncryptionKey",
    "secrets.getSigningKey", "secrets.getVerificationKey",
  ]);
  add("scripting.cache.service.CacheManagerScriptFacade", "assumed", [
    "cacheManager.exists", "cacheManager.named",
  ]);
  add("auth.nodes.script.ScriptedTreeWrapper", "assumed", [
    "journey.name", "journey.innerJourney", "journey.mustRun", "journey.identityResource",
  ]);
  add("scripting.wrappers.IdmIntegrationServiceScriptWrapper", "assumed", [
    "openidm.read", "openidm.query", "openidm.create", "openidm.update", "openidm.patch",
    "openidm.delete", "openidm.action",
  ]);
  add("scripting.wrappers.HttpClientScriptWrapper", "assumed", ["httpClient.send"]);
  add("scripting.bindings.SystemEnvScriptWrapper", "assumed", ["systemEnv.getProperty"]);
  add("scripting.logging.ScriptedLoggerWrapper", "assumed", ["logger.trace"]);
  // True varargs on AIC: 4, 5 and 6 arguments were accepted at all four levels
  // and every one bound a `{}` (E19 live probe, 2026-11-06). trace was not
  // probed, so it keeps the array-tail fault below.
  add("scripting.logging.ScriptedLoggerWrapper", "measured", [
    "logger.debug", "logger.info", "logger.warn", "logger.error",
  ]);
  // An overload whose last parameter introspection lists as `array` may be a
  // Java array or Java varargs (SLF4J's `Object...` is the obvious candidate
  // for logger's (format, arguments)), and introspection does not say which.
  // Rhino calls a varargs method with any count from one fewer than its
  // parameters upwards, a plain array parameter with exactly its count. So a
  // count at or above `arrayTail` that no listed overload takes is decided by
  // that reading, not by the overload set: unmeasured, and a fault.
  function tail(from, paths) {
    var i;
    for (i = 0; i < paths.length; i += 1) {
      basis[paths[i]].arrayTail = from;
    }
  }
  tail(0, ["utils.crypto.getRandomValues"]);
  tail(1, ["logger.trace"]);
  // Measured varargs: every count from `varargs` up is accepted.
  for (var v = 0, measured = ["logger.debug", "logger.info", "logger.warn", "logger.error"]; v < measured.length; v += 1) {
    basis[measured[v]].varargs = 1;
  }
  tail(2, ["openidm.read", "openidm.query", "openidm.patch"]);
  tail(3, ["openidm.delete", "callbacksBuilder.validatedUsernameCallback"]);
  tail(4, [
    "openidm.create", "openidm.update", "openidm.action",
    "callbacksBuilder.validatedPasswordCallback", "callbacksBuilder.stringAttributeInputCallback",
    "callbacksBuilder.numberAttributeInputCallback", "callbacksBuilder.booleanAttributeInputCallback",
  ]);
  return basis;
})();

// Rhino's NativeJavaMethod.scriptSignature: how a refused call names its
// arguments. After null and the primitive types, a Scriptable is Undefined,
// else a Wrapper named by unwrap().getClass().getName() in Java, else a
// Function ("function"), else "object". The Wrapper and Function checks run
// in Java (HostOps "scriptSignatureName"), against those interfaces, and on
// the JVM that answer is taken before the script-only function shortcut
// (Sol ctx r11 P2-2). A constructor such as java.lang.String is typeof
// "function" and a Wrapper whose unwrap is java.lang.Class, so the name is
// java.lang.Class. A method reference is a Function and not a Wrapper, so it
// stays "function". A regexp is typeof "object" and implements
// org.mozilla.javascript.Function (NativeRegExpCallable), so it is
// "function". That Java call does not consult the class shutter.
// Script-visible getClass() reflects java.lang.Class, which the class shutter
// refuses, so the overload message itself would become
// "Access to Java class \"java.lang.Class\" is prohibited." A plain script
// object is "object". Without the host (node:vm) the interface check cannot
// run: a function stays "function" and a regexp stays "object". The host
// function is captured here, before the runner deletes the __rhinoLocalHostOp
// global.
var __rhinoLocalScriptSignature = (function (host) {
  return function __rhinoLocalScriptSignature(args) {
    // Primitive concatenation only: a refusal is built before its fault is
    // recorded, so nothing here may call a builtin a script can replace
    // (Array.prototype.push/join, String).
    var out = "";
    var i;
    var value;
    var javaName;
    for (i = 0; i < args.length; i += 1) {
      value = args[i];
      if (i > 0) {
        out += ",";
      }
      if (value === null) {
        out += "null";
      } else if (value === undefined) {
        // Rhino 1.7.14's Undefined.instance is not a Scriptable, so it is named
        // by its Java class: AIC's `mergeShared(org.mozilla.javascript.Undefined).`
        // (E19 live probe, 2026-11-06).
        out += "org.mozilla.javascript.Undefined";
      } else if (typeof value === "boolean" || typeof value === "string" || typeof value === "number") {
        out += typeof value;
      } else if (host !== null) {
        // Wrapper before Function, both in Java. A constructor is typeof
        // "function" and a Wrapper; asking here is what names it
        // java.lang.Class. Null means the value is not a Java instance.
        javaName = host("scriptSignatureName", value);
        if (javaName !== null && javaName !== undefined) {
          out += "" + javaName;
        } else if (typeof value === "function") {
          out += "function";
        } else {
          out += "object";
        }
      } else if (typeof value === "function") {
        out += "function";
      } else {
        out += "object";
      }
    }
    return out;
  };
})(typeof __rhinoLocalHostOp === "function" ? __rhinoLocalHostOp : null);

function __rhinoLocalInternalError(message) {
  if (typeof InternalError === "function") {
    return new InternalError(message);
  }
  var error = new Error(message);
  error.name = "InternalError";
  return error;
}

// What a call with an argument count the local binding does not take throws:
// AIC's own refusal when the range has a basis, otherwise a harness fault. A
// count only a varargs reading of a trailing array parameter would accept has
// no basis either way (see `arrayTail`).
function __rhinoLocalArityRefusal(path, args, expected) {
  var basis = __rhinoLocalArityBasis[path];
  if (basis !== undefined && basis.arrayTail !== undefined && args.length >= basis.arrayTail) {
    return __rhinoLocalFault("arity-unknown/" + path,
      "rhino-local: " + path + " arity=" + args.length + " (AIC's introspection lists " + expected +
        " ending in an array parameter; whether AIC takes it as varargs, and so accepts this count, is unmeasured)"
    );
  }
  if (basis !== undefined) {
    return __rhinoLocalInternalError(
      "Can't find method " + basis.java + "." + path.substring(path.lastIndexOf(".") + 1) +
        "(" + __rhinoLocalScriptSignature(args) + ")."
    );
  }
  return __rhinoLocalFault("arity-unknown/" + path,
    "rhino-local: " + path + " arity=" + args.length + " (the harness accepts " + expected +
      "; AIC's accepted argument counts here are unmeasured)"
  );
}

function __rhinoLocalExpectArity(path, args, expected) {
  if (args.length !== expected) {
    throw __rhinoLocalArityRefusal(path, args, String(expected));
  }
}

function __rhinoLocalHas(object, key) {
  return Object.prototype.hasOwnProperty.call(object, key);
}

function __rhinoLocalIsPlainObject(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value)
  );
}

function __rhinoLocalAssignPlain(target, source) {
  if (!__rhinoLocalIsPlainObject(source)) {
    return;
  }
  source = __rhinoLocalEntries(source);
  var keys = Object.keys(source);
  var i;
  for (i = 0; i < keys.length; i += 1) {
    target[keys[i]] = source[keys[i]];
  }
}

var __rhinoLocalNativeArrayHash = 0x69fd8b92;

// A non-object merge throws AIC's catchable InternalError and changes no
// state (E19 live probe, 2026-11-06; docs/api/12): a string, number, boolean
// or undefined is Rhino's `Can't find method ...NodeStateScriptWrapper.<m>(<type>).`,
// null is accepted by overload resolution and fails inside as `null`, and an
// array resolves to the Map overload and fails conversion. Measured on
// mergeShared, and on mergeTransient for a string; the same wrapper's other
// mergeTransient cases are taken to match where the refusal is Rhino's own
// (overload resolution, Map conversion). mergeTransient(null) is not: null
// gets past resolution and fails inside AM's method body, which was never
// observed for mergeTransient, so it stays the fault (review pf-01 P3-3), as
// does anything else (a function).
function __rhinoLocalMergeRefusal(method, object) {
  if (object === null && method === "mergeShared") {
    return __rhinoLocalInternalError("null");
  }
  if (Array.isArray(object)) {
    __rhinoLocalNativeArrayHash = (__rhinoLocalNativeArrayHash + 0x9e3779b1) >>> 0;
    return __rhinoLocalInternalError("Cannot convert org.mozilla.javascript.NativeArray@" +
      __rhinoLocalNativeArrayHash.toString(16) + " to java.util.Map");
  }
  if (object === undefined || typeof object === "string" || typeof object === "number" ||
      typeof object === "boolean") {
    return __rhinoLocalInternalError("Can't find method org.forgerock.openam.auth.node.api.NodeStateScriptWrapper." +
      method + "(" + __rhinoLocalScriptSignature([object]) + ").");
  }
  return __rhinoLocalFault("node-state-merge-non-object",
    "rhino-local: nodeState." + method + ": what AIC does with a " + (object === null ? "null" : typeof object) + " argument is unmeasured");
}

function __rhinoLocalMergeBucket(bucketName, object) {
  if (!__rhinoLocalIsPlainObject(object)) {
    throw __rhinoLocalMergeRefusal(bucketName === "shared" ? "mergeShared" : "mergeTransient", object);
  }
  var bucket = __rhinoLocal[bucketName];
  var keys = Object.keys(object);
  var i;
  var key;
  var incoming;
  var existing;
  var merged;
  // Both merges refuse a nested plain object (measured), except inside
  // objectAttributes (measured for mergeShared). Arrays were not measured.
  for (i = 0; i < keys.length; i += 1) {
    if (
      keys[i] !== "objectAttributes" &&
      __rhinoLocalIsPlainObject(object[keys[i]])
    ) {
      throw new InternalError(
        "State must not contain nested objects unless they are inside registered state containers: objectAttributes"
      );
    }
  }
  for (i = 0; i < keys.length; i += 1) {
    key = keys[i];
    incoming = object[key];
    existing = bucket[key];
    if (__rhinoLocalIsPlainObject(existing) && __rhinoLocalIsPlainObject(incoming)) {
      merged = {};
      __rhinoLocalAssignPlain(merged, existing);
      __rhinoLocalAssignPlain(merged, incoming);
      bucket[key] = __rhinoLocalAsJavaMap(merged);
    } else if (__rhinoLocalIsPlainObject(incoming)) {
      // A merged object is stored as a Java map (measured); putShared's is not.
      merged = {};
      __rhinoLocalAssignPlain(merged, incoming);
      bucket[key] = __rhinoLocalAsJavaMap(merged);
    } else {
      bucket[key] = incoming;
    }
  }
}

// The stand-in, where there is no JVM, for a Java collection the decision
// node's own services hand over (node state keys, a map's values, an identity
// attribute): a JS array carrying the collection's methods. On the JVM each
// of those is the real java.util collection.
function __rhinoLocalJavaList(items) {
  var list = [];
  var i;
  for (i = 0; i < items.length; i += 1) {
    list.push(items[i]);
  }
  __rhinoLocalHide(list, "size", function () {
    return this.length;
  });
  __rhinoLocalHide(list, "get", function (index) {
    return this[index];
  });
  __rhinoLocalHide(list, "isEmpty", function () {
    return this.length === 0;
  });
  __rhinoLocalHide(list, "contains", function (value) {
    return this.indexOf(value) !== -1;
  });
  __rhinoLocalHide(list, "toArray", function () {
    return __rhinoLocalArrayLike(this);
  });
  return list;
}

// An unmeasured call's argument list for a refusal message, built before the
// fault is recorded and so without running anything a script can replace:
// no argument's toString, no String(), no Array.prototype method. A number
// primitive concatenates without calling out; everything else is its typeof.
function __rhinoLocalArgShape(args) {
  var shape = "";
  var k;
  var a;
  for (k = 0; k < args.length; k += 1) {
    a = args[k];
    shape += (k === 0 ? "" : ", ") + (typeof a === "number" ? "" + a : a === null ? "null" : typeof a);
  }
  return shape;
}

// A list AIC hands a script as a Java list is a real java.util.ArrayList
// here (java/JavaCollections.java), presented the way AIC presents one
// (java/SequenceFacade.java): `[object JavaList]`, `length` and `[i]`,
// size(), get(i), contains(), indexOf(), isEmpty(), add(), toArray() (a Java
// array, not a JS one), forEach, `[].concat(v)`, JSON.stringify and
// String(v), with the engine's own answer for each at every size, the empty
// list included. iterator() and getClass() are the class shutter's refusals
// because the classes they return are off the allow-list, not because this
// file says so. An Array.prototype method is `Cannot find function`.
//
// A script string goes in as a Java string and reads back as a JS string. A
// record the emulator holds as a script object goes in as it is, and
// String(v) writes it the way Java prints a map (`[{_id=…, _rev=…}]`,
// measured on an openidm query result).
//
// java/HostOps.java is defined for the prelude only and is captured here.
// The node:vm loader the unit tests use has no JVM: there a list is a plain
// array carrying the same method names, which stands in for the list in
// tests of everything else and is not a statement about AIC.
var __rhinoLocalJavaListHost = (function (hostOp) {
  if (typeof hostOp !== "function") {
    return null;
  }
  return {
    list: function (items, immutable) { return hostOp("javaList", items, immutable === true); },
    items: function (value) { return hostOp("javaListItems", value); },
    set: function (items) { return hostOp("javaSet", items); },
  };
})(typeof __rhinoLocalHostOp === "function" ? __rhinoLocalHostOp : null);

var __rhinoLocalListKey = "__rhinoLocalList";

function __rhinoLocalJavaForm(value) {
  var parts = [];
  var i;
  var keys;
  if (Array.isArray(value)) {
    for (i = 0; i < value.length; i += 1) {
      parts.push(__rhinoLocalJavaForm(value[i]));
    }
    return "[" + parts.join(", ") + "]";
  }
  if (value !== null && typeof value === "object" && Object.prototype.toString.call(value) === "[object Object]") {
    keys = Object.keys(value);
    for (i = 0; i < keys.length; i += 1) {
      parts.push(keys[i] + "=" + __rhinoLocalJavaForm(value[keys[i]]));
    }
    return "{" + parts.join(", ") + "}";
  }
  return String(value);
}

// The list a test without a JVM gets: an array carrying the Java list's own
// members, with every Array.prototype method a Java list lacks shadowed away.
function __rhinoLocalListDouble(items) {
  var list = items.slice();
  var names = Object.getOwnPropertyNames(Array.prototype);
  var i;
  for (i = 0; i < names.length; i += 1) {
    if (names[i] !== "constructor" && names[i] !== "length" && names[i] !== "forEach" && names[i] !== "indexOf") {
      __rhinoLocalHide(list, names[i], undefined);
    }
  }
  __rhinoLocalHide(list, "size", function () { return this.length; });
  __rhinoLocalHide(list, "get", function (index) {
    if (index < 0 || index >= this.length) {
      throw __rhinoLocalInternalError("Index " + index + " out of bounds for length " + this.length);
    }
    return this[index];
  });
  __rhinoLocalHide(list, "isEmpty", function () { return this.length === 0; });
  __rhinoLocalHide(list, "contains", function (value) { return Array.prototype.indexOf.call(this, value) !== -1; });
  __rhinoLocalHide(list, "add", function (value) { Array.prototype.push.call(this, value); return true; });
  __rhinoLocalHide(list, "toArray", function () { return Array.prototype.slice.call(this); });
  __rhinoLocalHide(list, "toString", function () { return __rhinoLocalJavaForm(Array.prototype.slice.call(this)); });
  __rhinoLocalHide(list, __rhinoLocalListKey, list);
  return list;
}

// `immutable` true is a list that refuses writes, as Java's unmodifiable
// list does (measured on accessToken.getScope(): add() is `InternalError:
// null`).
function __rhinoLocalListView(items, immutable) {
  var list;
  if (__rhinoLocalJavaListHost !== null) {
    return __rhinoLocalJavaListHost.list(items, immutable === true);
  }
  list = __rhinoLocalListDouble(items);
  if (immutable === true) {
    __rhinoLocalHide(list, "add", function () {
      throw __rhinoLocalInternalError("null");
    });
  }
  return list;
}

// The elements of a list this harness handed out, as a new JS array, or null
// when `value` is not one. The list is read as it is now, so what a script
// added is included.
function __rhinoLocalListItems(value) {
  var own;
  if (value === null || value === undefined || (typeof value !== "object" && typeof value !== "function")) {
    return null;
  }
  if (__rhinoLocalJavaListHost !== null) {
    return __rhinoLocalJavaListHost.items(value);
  }
  own = Object.getOwnPropertyDescriptor(value, __rhinoLocalListKey);
  return own !== undefined && own.value === value ? Array.prototype.slice.call(value) : null;
}


// A replacement for a Function.prototype builtin, with the builtin's own
// metadata: its name, its length, no own `prototype` and no constructor. On
// the JVM the runner supplies __rhinoLocalNativeMethodHost
// (java/NativeMethod.java) for the prelude, because a JavaScript function's
// `prototype` cannot be removed; elsewhere (node:vm) a method definition
// already has exactly that shape. `body` declares the builtin's length.
// Each replacement remembers its builtin, so the toString wrapper below
// renders it as the builtin.
var __rhinoLocalNativeMethods = [];
function __rhinoLocalNativeMethod(name, original, body) {
  var method;
  if (typeof __rhinoLocalNativeMethodHost === "function") {
    method = __rhinoLocalNativeMethodHost(name, body.length, body);
  } else if (name === "bind") {
    method = ({ bind(thisArg) { return body.apply(this, arguments); } }).bind;
  } else if (name === "toString") {
    method = ({ toString() { return body.apply(this, arguments); } }).toString;
  } else if (name === "toSource") {
    method = ({ toSource(indent) { return body.apply(this, arguments); } }).toSource;
  } else {
    throw new Error("rhino-local: no native-shaped method for " + name);
  }
  __rhinoLocalNativeMethods.push({ method: method, original: original });
  return method;
}

// Function.prototype.bind and toString borrowed onto a detail stub (the
// stub's own bind and toString already refuse): a bound copy would escape
// every guard on the stub, and the borrowed toString renders it. Any other
// receiver goes to the builtin.
(function () {
  "use strict";
  // Not named bind/toString: each body's own name would shadow them.
  var nativeBind = Function.prototype.bind;
  var nativeToString = Function.prototype.toString;
  __rhinoLocalHide(Function.prototype, "bind", __rhinoLocalNativeMethod("bind", nativeBind, function bind(thisArg) {
    var row = __rhinoLocalStubRow(this);
    if (row !== null && row.unmeasured) {
      throw __rhinoLocalDetailUnmeasured("." + row.name + ".bind(…)");
    }
    return nativeBind.apply(this, arguments);
  }));
  __rhinoLocalHide(Function.prototype, "toString", __rhinoLocalNativeMethod("toString", nativeToString, function toString() {
    var row = __rhinoLocalStubRow(this);
    var i;
    if (row !== null) {
      if (row.text !== null) {
        return row.text;
      }
      throw __rhinoLocalDetailUnmeasured("." + row.name + " as a string");
    }
    for (i = 0; i < __rhinoLocalNativeMethods.length; i += 1) {
      if (__rhinoLocalNativeMethods[i].method === this) {
        return nativeToString.call(__rhinoLocalNativeMethods[i].original);
      }
    }
    return nativeToString.apply(this, arguments);
  }));
  // Rhino's own Function.prototype.toSource (absent on node:vm) renders the
  // same member through its implementation, which is emulator source. Nothing
  // measured says what AIC prints, not even for asString (only String(asString)
  // was captured), so every branded member refuses.
  var nativeToSource = Function.prototype.toSource;
  if (typeof nativeToSource === "function") {
    __rhinoLocalHide(Function.prototype, "toSource", __rhinoLocalNativeMethod("toSource", nativeToSource, function toSource(indent) {
      var row = __rhinoLocalStubRow(this);
      var i;
      if (row !== null) {
        throw __rhinoLocalDetailUnmeasured("." + row.name + " as source");
      }
      for (i = 0; i < __rhinoLocalNativeMethods.length; i += 1) {
        if (__rhinoLocalNativeMethods[i].method === this) {
          return nativeToSource.apply(__rhinoLocalNativeMethods[i].original, arguments);
        }
      }
      return nativeToSource.apply(this, arguments);
    }));
  }
})();

// The serialisation boundary: every value native JSON.stringify is about to
// write passes here, after its toJSON and the author's replacer, so a detail
// stub faults. A property-list replacer goes to native unchanged.
function __rhinoLocalJsonValue(value) {
  var stub = __rhinoLocalStubRow(value);
  if (stub !== null && stub.unmeasured) {
    throw __rhinoLocalDetailUnmeasured("." + stub.name + " under JSON.stringify");
  }
  return value;
}

(function () {
  "use strict";
  var original = JSON.stringify;
  __rhinoLocalHide(JSON, "stringify", function stringify(value, replacer, space) {
    if (Array.isArray(replacer)) {
      return original.call(JSON, value, replacer, space);
    }
    var author = typeof replacer === "function" ? replacer : null;
    return original.call(JSON, value, function (key, v) {
      return __rhinoLocalJsonValue(author === null ? v : author.call(this, key, v));
    }, space);
  });
})();

// A request map's value list.
function __rhinoLocalRequestList(items) {
  return __rhinoLocalListView(items);
}

// An openidm query result or a has-many relationship array on a read.
function __rhinoLocalOpenidmList(items) {
  return __rhinoLocalListView(items);
}


function __rhinoLocalRequestMap(values, options) {
  var caseInsensitive = Boolean(options && options.caseInsensitive);
  var asList = !(options && options.asList === false);
  var label = (options && options.label) || "request map";
  var makeList = __rhinoLocalRequestList;
  var binding = label === "requestHeaders" || label === "requestParameters" ? label : "requestMap";
  var map = {};
  var store = {};
  var order = [];
  var keyCount = 0;
  var names = values ? Object.keys(values) : [];
  var i;
  var raw;
  var stored;
  var value;
  for (i = 0; i < names.length; i += 1) {
    raw = names[i];
    stored = caseInsensitive ? String(raw).toLowerCase() : String(raw);
    value = values[raw];
    if (asList) {
      if (typeof value === "string") {
        store[stored] = makeList([value], binding);
      } else {
        store[stored] = makeList(value, binding);
      }
    } else {
      store[stored] = value;
    }
    map[stored] = store[stored];
    if (order.indexOf(stored) === -1) {
      order.push(stored);
    }
    keyCount += 1;
  }
  __rhinoLocalHide(map, "get", function (name) {
    __rhinoLocalExpectArity("map.get", arguments, 1);
    var key = caseInsensitive ? String(name).toLowerCase() : String(name);
    if (!__rhinoLocalHas(store, key)) {
      return null;
    }
    return store[key];
  });
  __rhinoLocalHide(map, "containsKey", function (name) {
    __rhinoLocalExpectArity("map.containsKey", arguments, 1);
    var key = caseInsensitive ? String(name).toLowerCase() : String(name);
    return __rhinoLocalHas(store, key);
  });
  __rhinoLocalHide(map, "size", function () {
    return keyCount;
  });
  __rhinoLocalHide(map, "isEmpty", function () {
    return keyCount === 0;
  });
  __rhinoLocalHide(map, "keySet", function () {
    throw __rhinoLocalEmulated(
      "rhino-local: keySet is blocked by AM's class shutter; iterate with for...in"
    );
  });
  // Measured on requestHeaders and requestParameters: the class shutter's refusal.
  __rhinoLocalHide(map, "getClass", function () {
    throw __rhinoLocalShutterError("java.lang.Class");
  });
  // java.util.Map.forEach(BiConsumer), measured 2026-11-08 on all three request
  // maps (docs/api/12, request-map-foreach probe): the callback gets exactly
  // (key, value) with `this` not the map, forEach returns undefined, and a
  // throw inside the callback reaches the caller as the same error. No
  // argument, or a number, is Rhino's overload refusal. Key order on AIC is the
  // map's hash order and is not reproduced: seeding order is used, as for
  // existingSession. Anything else passed is unmeasured.
  __rhinoLocalHide(map, "forEach", function (action) {
    var k;
    if (arguments.length === 1 && typeof action === "function") {
      for (k = 0; k < order.length; k += 1) {
        action.call(undefined, order[k], store[order[k]]);
      }
      return undefined;
    }
    if (arguments.length === 0 || (arguments.length === 1 && typeof action === "number")) {
      throw __rhinoLocalInternalError("Can't find method java.util.Map.forEach(" +
        __rhinoLocalScriptSignature(arguments) + ").");
    }
    throw __rhinoLocalFault("request-map-foreach-argument",
      "rhino-local: " + label + ".forEach(" + __rhinoLocalScriptSignature(arguments) +
        "): AIC's answer is measured for one function, no argument, or a number only");
  });
  return __rhinoLocalGuardService(label, map);
}

/**
 * `existingSession`: a String->String map of AM's own session properties plus
 * anything a prior journey stored with `putSessionProperty`. Present only when
 * the request carries a session cookie. Measured 2026-09-14 against a live
 * tenant (docs/api/12-script-bindings-matrix.md).
 *
 * Deliberately NOT a plain object; each difference is one a plain object would
 * get wrong in the false-pass direction:
 *   - `hasOwnProperty` is not reachable on AM's wrapper, so a script that calls
 *     it passes here and throws there. Hence the null prototype.
 *   - `keySet()` exists and is a trap: the returned java.util.HashMap$KeySet is
 *     refused by AM's class shutter. Iterate with for...in, as the request maps
 *     above already require.
 *   - `String(session)` wraps the map in MapScriptWrapper, whose toString is
 *     JSON-ish. Key ORDER is AM's hash order and is not reproduced -- seeding
 *     order is used instead.
 */
function __rhinoLocalSessionMap(values) {
  var map = typeof Object.create === "function" ? Object.create(null) : {};
  var names = values ? Object.keys(values) : [];
  var i;
  for (i = 0; i < names.length; i += 1) {
    map[String(names[i])] = String(values[names[i]]);
  }
  __rhinoLocalHide(map, "get", function (key) {
    __rhinoLocalExpectArity("existingSession.get", arguments, 1);
    var k = String(key);
    if (!__rhinoLocalHas(map, k)) {
      return null;
    }
    return map[k];
  });
  __rhinoLocalHide(map, "put", function (key, value) {
    __rhinoLocalExpectArity("existingSession.put", arguments, 2);
    map[String(key)] = String(value);
  });
  __rhinoLocalHide(map, "containsKey", function (key) {
    __rhinoLocalExpectArity("existingSession.containsKey", arguments, 1);
    return __rhinoLocalHas(map, String(key));
  });
  __rhinoLocalHide(map, "size", function () {
    return __rhinoLocalSessionKeys(map).length;
  });
  __rhinoLocalHide(map, "isEmpty", function () {
    return __rhinoLocalSessionKeys(map).length === 0;
  });
  __rhinoLocalHide(map, "keySet", function () {
    throw __rhinoLocalEmulated(
      "rhino-local: existingSession.keySet is blocked by AM's class shutter " +
        '(Access to Java class "java.util.HashMap$KeySet" is prohibited); ' +
        "iterate with for...in"
    );
  });
  __rhinoLocalHide(map, "toString", function () {
    var keys = __rhinoLocalSessionKeys(map);
    var parts = [];
    var j;
    for (j = 0; j < keys.length; j += 1) {
      parts.push(
        JSON.stringify(keys[j]) + ": " + JSON.stringify(map[keys[j]])
      );
    }
    return "{ " + parts.join(", ") + " }";
  });
  return __rhinoLocalGuardService("existingSession", map);
}

function __rhinoLocalSessionKeys(map) {
  var out = [];
  var k;
  for (k in map) {
    if (__rhinoLocalHas(map, k)) {
      out.push(k);
    }
  }
  return out;
}

function __rhinoLocalLookupState(key) {
  if (__rhinoLocalHas(__rhinoLocal.transient, key)) {
    return __rhinoLocal.transient[key];
  }
  if (__rhinoLocalHas(__rhinoLocal.secure, key)) {
    return __rhinoLocal.secure[key];
  }
  if (__rhinoLocalHas(__rhinoLocal.shared, key)) {
    return __rhinoLocal.shared[key];
  }
  return null;
}

function __rhinoLocalIsDefined(key) {
  return (
    __rhinoLocalHas(__rhinoLocal.transient, key) ||
    __rhinoLocalHas(__rhinoLocal.secure, key) ||
    __rhinoLocalHas(__rhinoLocal.shared, key)
  );
}

nodeState.get = function (key) {
  __rhinoLocalExpectArity("nodeState.get", arguments, 1);
  var name = String(key);
  return __rhinoLocalLookupState(name);
};

nodeState.isDefined = function (key) {
  __rhinoLocalExpectArity("nodeState.isDefined", arguments, 1);
  return __rhinoLocalIsDefined(String(key));
};

nodeState.putShared = function (key, value) {
  __rhinoLocalExpectArity("nodeState.putShared", arguments, 2);
  if (value === undefined) {
    throw __rhinoLocalFault("state-put-undefined", "rhino-local: nodeState.putShared: value is undefined");
  }
  __rhinoLocal.shared[String(key)] = value;
  return nodeState;
};

nodeState.putTransient = function (key, value) {
  __rhinoLocalExpectArity("nodeState.putTransient", arguments, 2);
  if (value === undefined) {
    throw __rhinoLocalFault("state-put-undefined", "rhino-local: nodeState.putTransient: value is undefined");
  }
  __rhinoLocal.transient[String(key)] = value;
  return nodeState;
};

nodeState.remove = function (key) {
  __rhinoLocalExpectArity("nodeState.remove", arguments, 1);
  var k = String(key);
  delete __rhinoLocal.shared[k];
  delete __rhinoLocal.transient[k];
  delete __rhinoLocal.secure[k];
};

// Every key nodeState holds, transient first, as a plain array.
function __rhinoLocalNodeStateKeyNames() {
  var seen = {};
  var names = [];
  function take(bucket) {
    var keys = Object.keys(bucket);
    var i;
    var key;
    for (i = 0; i < keys.length; i += 1) {
      key = keys[i];
      if (!__rhinoLocalHas(seen, key)) {
        seen[key] = true;
        names.push(key);
      }
    }
  }
  take(__rhinoLocal.transient);
  take(__rhinoLocal.secure);
  take(__rhinoLocal.shared);
  return names;
}

// Measured: a Java set, iterated with iterator(). On the JVM it is a real
// java.util.HashSet, so every member and the order are Java's own; the array
// below stands in where there is no JVM.
nodeState.keys = function () {
  __rhinoLocalExpectArity("nodeState.keys", arguments, 0);
  if (__rhinoLocalJavaListHost !== null) {
    return __rhinoLocalJavaListHost.set(__rhinoLocalNodeStateKeyNames());
  }
  var list = __rhinoLocalJavaList(__rhinoLocalNodeStateKeyNames());
  __rhinoLocalHide(list, "iterator", function () {
    var index = 0;
    return {
      hasNext: function () {
        return index < list.length;
      },
      next: function () {
        return list[index++];
      },
    };
  });
  return list;
};

// How a Java map/list from nodeState.getObject prints (measured).
function __rhinoLocalJavaMapString(value) {
  if (value === null) {
    return "null";
  }
  if (Array.isArray(value)) {
    var items = [];
    var i;
    for (i = 0; i < value.length; i += 1) {
      items.push(__rhinoLocalJavaMapString(value[i]));
    }
    return "[ " + items.join(", ") + " ]";
  }
  if (__rhinoLocalIsPlainObject(value)) {
    value = __rhinoLocalEntries(value);
    var keys = Object.keys(value);
    var pairs = [];
    var j;
    for (j = 0; j < keys.length; j += 1) {
      pairs.push(JSON.stringify(keys[j]) + ": " + __rhinoLocalJavaMapString(value[keys[j]]));
    }
    return "{ " + pairs.join(", ") + " }";
  }
  if (typeof value === "string") {
    return JSON.stringify(value);
  }
  if (typeof value === "number" && Math.floor(value) === value) {
    return String(value) + ".0";
  }
  return String(value);
}

// A plain object dressed as the Java map AIC hands back: it prints as one,
// and containsKey, size and keySet are functions (measured). Every entry is an
// ordinary enumerable property, so dot access, `map[k]`, for...in and
// Object.keys all see it (measured 2026-10-01, live-java-map-enumeration).
// That includes an entry named like a method: on AIC it SHADOWS the method, so
// with a "get" entry `map.get` is the entry's value and `map.get(k)` throws
// "is not a function" (measured, same probe). The harness reads entries
// through the hidden __rhinoLocalMapEntries, which no script key can shadow.
function __rhinoLocalShutterError(className) {
  var error = new Error('Access to Java class "' + className + '" is prohibited.');
  error.name = "InternalError";
  return error;
}

function __rhinoLocalEntries(value) {
  if (value && typeof value.__rhinoLocalMapEntries === "function") {
    return value.__rhinoLocalMapEntries();
  }
  return value;
}

function __rhinoLocalAsJavaMap(source) {
  var map = {};
  // Insertion order. An entry assigned by dot access (map.x = 1) is picked up
  // after these.
  var order = [];
  function has(key) {
    return Object.prototype.propertyIsEnumerable.call(map, key);
  }
  function keys() {
    var extra = Object.keys(map).filter(function (key) {
      return order.indexOf(key) === -1;
    });
    return order.filter(has).concat(extra);
  }
  function set(key, value) {
    if (!has(key) && order.indexOf(key) === -1) {
      order.push(key);
    }
    // defineProperty, because a method-named key replaces a hidden
    // (non-enumerable) method and must become an enumerable entry.
    Object.defineProperty(map, key, {
      value: value,
      enumerable: true,
      writable: true,
      configurable: true
    });
  }
  function entries() {
    var out = {};
    var names = keys();
    var i;
    for (i = 0; i < names.length; i += 1) {
      out[names[i]] = map[names[i]];
    }
    return out;
  }
  function values() {
    var names = keys();
    var out = [];
    var i;
    for (i = 0; i < names.length; i += 1) {
      out.push(map[names[i]]);
    }
    return out;
  }
  __rhinoLocalHide(map, "__rhinoLocalMapEntries", entries);
  __rhinoLocalHide(map, "toJSON", entries);
  __rhinoLocalHide(map, "toString", function () {
    return __rhinoLocalJavaMapString(map);
  });
  __rhinoLocalHide(map, "get", function (name) {
    var key = String(name);
    return has(key) ? map[key] : null;
  });
  __rhinoLocalHide(map, "put", function (name, value) {
    var key = String(name);
    var previous = has(key) ? map[key] : null;
    set(key, value);
    return previous;
  });
  __rhinoLocalHide(map, "containsKey", function (name) {
    return has(String(name));
  });
  __rhinoLocalHide(map, "size", function () {
    return keys().length;
  });
  // keySet() itself is refused: AM's class shutter blocks the
  // LinkedHashMap$LinkedKeySet it returns (measured). Iterate with for...in.
  __rhinoLocalHide(map, "keySet", function () {
    throw __rhinoLocalShutterError("java.util.LinkedHashMap$LinkedKeySet");
  });
  // Measured: a snapshot. size(), iterator().hasNext() and toArray().length
  // work; an entry itself is a LinkedHashMap$Entry, which the class shutter
  // refuses, so next() and reading a toArray() element throw. Nothing else
  // on the view is modelled.
  __rhinoLocalHide(map, "entrySet", function () {
    var size = keys().length;
    var view = {};
    function refused() {
      throw __rhinoLocalShutterError("java.util.LinkedHashMap$Entry");
    }
    __rhinoLocalHide(view, "size", function () {
      return size;
    });
    __rhinoLocalHide(view, "iterator", function () {
      var seen = 0;
      var iterator = {};
      __rhinoLocalHide(iterator, "hasNext", function () {
        return seen < size;
      });
      __rhinoLocalHide(iterator, "next", refused);
      return iterator;
    });
    __rhinoLocalHide(view, "toArray", function () {
      var array = [];
      var i;
      for (i = 0; i < size; i += 1) {
        Object.defineProperty(array, String(i), { get: refused, enumerable: true });
      }
      return array;
    });
    return view;
  });
  // Measured: a snapshot list; size(), get(i) and toArray() work, iterator()
  // is refused (ArrayList$Itr). On the JVM it is a real java.util.ArrayList
  // and the refusal is the class shutter's.
  __rhinoLocalHide(map, "values", function () {
    if (__rhinoLocalJavaListHost !== null) {
      return __rhinoLocalListView(values());
    }
    var list = __rhinoLocalJavaList(values());
    __rhinoLocalHide(list, "iterator", function () {
      throw __rhinoLocalShutterError("java.util.ArrayList$Itr");
    });
    return list;
  });
  var names = Object.keys(source || {});
  var i;
  for (i = 0; i < names.length; i += 1) {
    set(names[i], source[names[i]]);
  }
  return map;
}

nodeState.getObject = function (key) {
  __rhinoLocalExpectArity("nodeState.getObject", arguments, 1);
  var k = String(key);
  var sharedVal = __rhinoLocalHas(__rhinoLocal.shared, k)
    ? __rhinoLocal.shared[k]
    : undefined;
  var secureVal = __rhinoLocalHas(__rhinoLocal.secure, k)
    ? __rhinoLocal.secure[k]
    : undefined;
  var transientVal = __rhinoLocalHas(__rhinoLocal.transient, k)
    ? __rhinoLocal.transient[k]
    : undefined;
  if (
    __rhinoLocalIsPlainObject(sharedVal) ||
    __rhinoLocalIsPlainObject(secureVal) ||
    __rhinoLocalIsPlainObject(transientVal)
  ) {
    var merged = {};
    __rhinoLocalAssignPlain(merged, sharedVal);
    __rhinoLocalAssignPlain(merged, secureVal);
    __rhinoLocalAssignPlain(merged, transientVal);
    return __rhinoLocalAsJavaMap(merged);
  }
  return __rhinoLocalLookupState(k);
};

nodeState.mergeShared = function (object) {
  __rhinoLocalExpectArity("nodeState.mergeShared", arguments, 1);
  __rhinoLocalMergeBucket("shared", object);
  return nodeState;
};

nodeState.mergeTransient = function (object) {
  __rhinoLocalExpectArity("nodeState.mergeTransient", arguments, 1);
  __rhinoLocalMergeBucket("transient", object);
  return nodeState;
};

// AIC's action is a Java wrapper; String(action) names its class (measured).
action.toString = function () {
  return "org.forgerock.openam.auth.nodes.script.ActionWrapper@1b6d3586";
};

action.goTo = function (name) {
  __rhinoLocalExpectArity("action.goTo", arguments, 1);
  __rhinoLocal.outcome = String(name);
  outcome = __rhinoLocal.outcome;
  return action;
};

action.withErrorMessage = function (message) {
  __rhinoLocalExpectArity("action.withErrorMessage", arguments, 1);
  return action;
};

action.withHeader = function (header) {
  __rhinoLocalExpectArity("action.withHeader", arguments, 1);
  return action;
};

action.withIdentifiedUser = function (username) {
  __rhinoLocalExpectArity("action.withIdentifiedUser", arguments, 1);
  return action;
};

action.withIdentifiedAgent = function (agentName) {
  __rhinoLocalExpectArity("action.withIdentifiedAgent", arguments, 1);
  return action;
};

action.withStage = function (stage) {
  __rhinoLocalExpectArity("action.withStage", arguments, 1);
  return action;
};

action.withDescription = function (description) {
  __rhinoLocalExpectArity("action.withDescription", arguments, 1);
  return action;
};

action.withLockoutMessage = function (lockoutMessage) {
  __rhinoLocalExpectArity("action.withLockoutMessage", arguments, 1);
  return action;
};

action.putSessionProperty = function (key, value) {
  __rhinoLocalExpectArity("action.putSessionProperty", arguments, 2);
  __rhinoLocal.sessionProperties[String(key)] = String(value);
  return action;
};

action.removeSessionProperty = function (key) {
  __rhinoLocalExpectArity("action.removeSessionProperty", arguments, 1);
  delete __rhinoLocal.sessionProperties[String(key)];
  return action;
};

// AIC's Integer parameter (measured): "120" and 1.5 convert, "x" does not,
// and null matches no overload.
function __rhinoLocalActionInteger(method, value) {
  if (value === null) {
    throw new InternalError(
      "Can't find method org.forgerock.openam.auth.nodes.script.ActionWrapper." +
        method +
        "(null)."
    );
  }
  if (isNaN(Number(value))) {
    throw new InternalError("Cannot convert " + value + " to java.lang.Integer");
  }
}

action.withMaxSessionTime = function (maxSessionTime) {
  __rhinoLocalExpectArity("action.withMaxSessionTime", arguments, 1);
  __rhinoLocalActionInteger("withMaxSessionTime", maxSessionTime);
  return action;
};

action.withMaxIdleTime = function (maxIdleTime) {
  __rhinoLocalExpectArity("action.withMaxIdleTime", arguments, 1);
  __rhinoLocalActionInteger("withMaxIdleTime", maxIdleTime);
  return action;
};

action.suspend = function (callbackTextFormat, additionalLogic) {
  if (arguments.length < 1 || arguments.length > 3) {
    throw __rhinoLocalArityRefusal("action.suspend", arguments, "1..3");
  }
  var message = String(callbackTextFormat);
  var resumeUri = "https://rhino-local.invalid/resume";
  if (arguments.length >= 2) {
    if (typeof additionalLogic !== "function") {
      throw __rhinoLocalFault("action-suspend-additional-logic", 
        "rhino-local: action.suspend additionalLogic is not a function"
      );
    }
    message = message.split("{0}").join(resumeUri);
    additionalLogic(resumeUri);
  }
  __rhinoLocalCallback("SuspendedTextOutputCallback", { message: message });
  return action;
};

function __rhinoLocalIsThrowable(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    typeof value.getMessage === "function" &&
    typeof value.getStackTrace === "function"
  );
}

function __rhinoLocalFormat(pattern, bound) {
  var out = "";
  var i = 0;
  var argIndex = 0;
  var slashes;
  var pairs;
  var odd;
  while (i < pattern.length) {
    if (
      pattern.charAt(i) === "{" &&
      i + 1 < pattern.length &&
      pattern.charAt(i + 1) === "}"
    ) {
      slashes = 0;
      while (out.length - slashes > 0 && out.charAt(out.length - 1 - slashes) === "\\") {
        slashes += 1;
      }
      pairs = Math.floor(slashes / 2);
      odd = slashes % 2 === 1;
      out = out.substring(0, out.length - slashes);
      var p;
      for (p = 0; p < pairs; p += 1) {
        out += "\\";
      }
      if (odd) {
        out += "{}";
      } else if (argIndex < bound.length) {
        out += __rhinoLocalLogArg(bound[argIndex]);
        argIndex += 1;
      } else {
        out += "{}";
      }
      i += 2;
    } else {
      out += pattern.charAt(i);
      i += 1;
    }
  }
  return out;
}

// How AIC's SLF4J back end prints a bound argument (E19 live probe,
// 2026-11-06): a JS number reaches it as a java.lang.Double, so 1 prints
// `1.0`; true, null and a plain object print `true`, `null` and
// `[object Object]`. Every other value keeps String(), unmeasured.
function __rhinoLocalJavaDouble(n) {
  if (n !== n) {
    return "NaN";
  }
  if (n === Infinity || n === -Infinity) {
    return n > 0 ? "Infinity" : "-Infinity";
  }
  if (n === 0) {
    return 1 / n < 0 ? "-0.0" : "0.0";
  }
  var abs = Math.abs(n);
  if (abs >= 1e-3 && abs < 1e7) {
    var plain = String(n);
    return plain.indexOf(".") < 0 ? plain + ".0" : plain;
  }
  var parts = n.toExponential().split("e");
  return (parts[0].indexOf(".") < 0 ? parts[0] + ".0" : parts[0]) + "E" + parts[1].replace("+", "");
}

function __rhinoLocalLogArg(value) {
  return typeof value === "number" ? __rhinoLocalJavaDouble(value) : String(value);
}

function __rhinoLocalLog(level, args) {
  var basis = __rhinoLocalArityBasis["logger." + level];
  var varargs = basis !== undefined && basis.varargs !== undefined && args.length >= basis.varargs;
  if (args.length < 1 || (args.length > 3 && !varargs)) {
    throw __rhinoLocalArityRefusal("logger." + level, args, "1..3");
  }
  var format = String(args[0]);
  var bound = [];
  var i;
  if (args.length === 2 && Array.isArray(args[1])) {
    bound = args[1];
  } else {
    for (i = 1; i < args.length; i += 1) {
      bound.push(args[i]);
    }
  }
  if (bound.length > 0 && __rhinoLocalIsThrowable(bound[bound.length - 1])) {
    bound = bound.slice(0, bound.length - 1);
  }
  __rhinoLocal.logs.push({
    level: level,
    message: __rhinoLocalFormat(format, bound),
  });
}

logger.getName = function () {
  return "scripts.AUTHENTICATION_TREE_DECISION_NODE." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
};

logger.trace = function () {
  __rhinoLocalLog("trace", arguments);
};

logger.debug = function () {
  __rhinoLocalLog("debug", arguments);
};

logger.info = function () {
  __rhinoLocalLog("info", arguments);
};

logger.warn = function () {
  __rhinoLocalLog("warn", arguments);
};

logger.error = function () {
  __rhinoLocalLog("error", arguments);
};

logger.isTraceEnabled = function () {
  return false;
};

logger.isDebugEnabled = function () {
  return true;
};

logger.isInfoEnabled = function () {
  return true;
};

logger.isWarnEnabled = function () {
  return true;
};

logger.isErrorEnabled = function () {
  return true;
};

function __rhinoLocalCallback(type, fields) {
  var cb = { type: type };
  var keys = Object.keys(fields);
  var i;
  var key;
  for (i = 0; i < keys.length; i += 1) {
    key = keys[i];
    if (fields[key] !== undefined) {
      cb[key] = fields[key];
    }
  }
  __rhinoLocal.callbacks.push(cb);
}

callbacksBuilder.nameCallback = function (prompt, defaultName) {
  if (arguments.length !== 1 && arguments.length !== 2) {
    throw __rhinoLocalArityRefusal("callbacksBuilder.nameCallback", arguments, "1 or 2");
  }
  __rhinoLocalCallback("NameCallback", { prompt: String(prompt) });
};

callbacksBuilder.passwordCallback = function (prompt, echoOn) {
  __rhinoLocalExpectArity("callbacksBuilder.passwordCallback", arguments, 2);
  __rhinoLocalCallback("PasswordCallback", { prompt: String(prompt) });
};

callbacksBuilder.textOutputCallback = function (messageType, message) {
  __rhinoLocalExpectArity("callbacksBuilder.textOutputCallback", arguments, 2);
  __rhinoLocalCallback("TextOutputCallback", {
    messageType: String(messageType),
    message: String(message),
  });
};

callbacksBuilder.hiddenValueCallback = function (id, value) {
  __rhinoLocalExpectArity("callbacksBuilder.hiddenValueCallback", arguments, 2);
  __rhinoLocalCallback("HiddenValueCallback", {
    id: String(id),
    value: String(value),
  });
};

callbacksBuilder.confirmationCallback = function () {
  var a = arguments;
  var fields = {};
  if (a.length === 3 && typeof a[0] === "number" && Array.isArray(a[1])) {
    fields.prompt = "";
    fields.messageType = a[0];
    fields.options = a[1];
    fields.optionType = -1;
    fields.defaultOption = a[2];
  } else if (a.length === 3 && typeof a[0] === "number") {
    fields.prompt = "";
    fields.messageType = a[0];
    fields.options = [];
    fields.optionType = a[1];
    fields.defaultOption = a[2];
  } else if (a.length === 4 && Array.isArray(a[2])) {
    fields.prompt = String(a[0]);
    fields.messageType = a[1];
    fields.options = a[2];
    fields.optionType = -1;
    fields.defaultOption = a[3];
  } else if (a.length === 4) {
    fields.prompt = String(a[0]);
    fields.messageType = a[1];
    fields.options = [];
    fields.optionType = a[2];
    fields.defaultOption = a[3];
  } else {
    throw __rhinoLocalArityRefusal("callbacksBuilder.confirmationCallback", a, "3 or 4");
  }
  __rhinoLocalCallback("ConfirmationCallback", fields);
};

callbacksBuilder.choiceCallback = function (
  prompt,
  choices,
  defaultChoice,
  multipleSelectionsAllowed
) {
  __rhinoLocalExpectArity("callbacksBuilder.choiceCallback", arguments, 4);
  __rhinoLocalCallback("ChoiceCallback", {
    prompt: String(prompt),
    choices: choices,
    defaultChoice: defaultChoice,
  });
};

callbacksBuilder.radioChoiceCallback = function (prompt, choices, defaultChoice) {
  __rhinoLocalExpectArity("callbacksBuilder.radioChoiceCallback", arguments, 3);
  __rhinoLocalCallback("ChoiceCallback", {
    prompt: String(prompt),
    choices: choices,
    defaultChoice: defaultChoice,
    radio: true,
  });
};

callbacksBuilder.suspendedTextOutputCallback = function (messageType, message) {
  __rhinoLocalExpectArity(
    "callbacksBuilder.suspendedTextOutputCallback",
    arguments,
    2
  );
  __rhinoLocalCallback("SuspendedTextOutputCallback", {
    messageType: String(messageType),
    message: String(message),
  });
};

callbacksBuilder.textInputCallback = function (prompt, defaultText) {
  if (arguments.length !== 1 && arguments.length !== 2) {
    throw __rhinoLocalArityRefusal("callbacksBuilder.textInputCallback", arguments, "1 or 2");
  }
  var textFields = { prompt: String(prompt) };
  textFields.defaultText = arguments.length > 1 ? String(defaultText) : "";
  __rhinoLocalCallback("TextInputCallback", textFields);
};

callbacksBuilder.scriptTextOutputCallback = function (message) {
  __rhinoLocalExpectArity(
    "callbacksBuilder.scriptTextOutputCallback",
    arguments,
    1
  );
  __rhinoLocalCallback("TextOutputCallback", {
    message: String(message),
    messageType: "4",
  });
};

callbacksBuilder.languageCallback = function (language, country) {
  __rhinoLocalExpectArity("callbacksBuilder.languageCallback", arguments, 2);
  __rhinoLocalCallback("LanguageCallback", {});
};

callbacksBuilder.idPCallback = function (
  provider,
  clientId,
  redirectUri,
  scope,
  nonce,
  request,
  requestUri,
  acrValues,
  requestNativeAppForUserInfo,
  token,
  tokenType
) {
  if (arguments.length !== 9 && arguments.length !== 11) {
    throw __rhinoLocalArityRefusal("callbacksBuilder.idPCallback", arguments, "9 or 11");
  }
  var idpFields = {
    provider: String(provider),
    clientId: String(clientId),
    redirectUri: String(redirectUri),
    scopes: scope,
    nonce: String(nonce),
    request: String(request),
    requestUri: String(requestUri),
    acrValues: acrValues,
    acceptsJSON: Boolean(requestNativeAppForUserInfo),
  };
  __rhinoLocalCallback("IdPCallback", idpFields);
};

callbacksBuilder.httpCallback = function () {
  var a = arguments;
  if (a.length === 4) {
    __rhinoLocalCallback("HttpCallback", {
      authRHeader: String(a[0]),
      negoName: String(a[1]),
      negoValue: String(a[2]),
      errorCode: a[3],
    });
    return;
  }
  if (a.length === 3) {
    __rhinoLocalCallback("HttpCallback", {
      authorizationHeader: String(a[0]),
      negotiationHeader: String(a[1]),
      errorCode: a[2],
    });
    return;
  }
  throw __rhinoLocalArityRefusal("callbacksBuilder.httpCallback", a, "3 or 4");
};

callbacksBuilder.x509CertificateCallback = function (
  prompt,
  certificate,
  requestSignature
) {
  if (arguments.length < 1 || arguments.length > 3) {
    throw __rhinoLocalArityRefusal("callbacksBuilder.x509CertificateCallback", arguments, "1..3");
  }
  var x509 = { prompt: String(prompt) };
  if (arguments.length > 1) {
    x509.certificate = certificate;
  }
  if (arguments.length > 2) {
    x509.requestSignature = Boolean(requestSignature);
  }
  __rhinoLocalCallback("X509CertificateCallback", x509);
};

callbacksBuilder.consentMappingCallback = function () {
  var a = arguments;
  if (a.length === 7) {
    __rhinoLocalCallback("ConsentMappingCallback", {
      name: String(a[0]),
      displayName: String(a[1]),
      icon: String(a[2]),
      accessLevel: String(a[3]),
      fields: a[4],
      message: String(a[5]),
      isRequired: Boolean(a[6]),
    });
    return;
  }
  if (a.length === 3) {
    var config = a[0];
    var emptyFields = [];
    var i;
    // AIC streams config.fields and fails without it (measured).
    if (!config || !Array.isArray(config.fields)) {
      throw __rhinoLocalEmulated(
        "rhino-local: consentMappingCallback config needs a fields list; AIC refuses one without"
      );
    }
    for (i = 0; i < config.fields.length; i += 1) {
      emptyFields.push(null);
    }
    __rhinoLocalCallback("ConsentMappingCallback", {
      name: String(config.name),
      displayName: String(config.displayName),
      icon: String(config.icon),
      accessLevel: String(config.accessLevel),
      fields: emptyFields,
      message: String(a[1]),
      isRequired: Boolean(a[2]),
    });
    return;
  }
  throw __rhinoLocalArityRefusal("callbacksBuilder.consentMappingCallback", a, "3 or 7");
};

callbacksBuilder.deviceProfileCallback = function (metadata, location, message) {
  __rhinoLocalExpectArity("callbacksBuilder.deviceProfileCallback", arguments, 3);
  __rhinoLocalCallback("DeviceProfileCallback", {
    metadata: Boolean(metadata),
    location: Boolean(location),
    message: String(message),
  });
};

callbacksBuilder.kbaCreateCallback = function (
  prompt,
  predefinedQuestions,
  allowUserDefinedQuestions
) {
  __rhinoLocalExpectArity("callbacksBuilder.kbaCreateCallback", arguments, 3);
  __rhinoLocalCallback("KbaCreateCallback", {
    prompt: String(prompt),
    predefinedQuestions: predefinedQuestions,
    allowUserDefinedQuestions: Boolean(allowUserDefinedQuestions),
  });
};

callbacksBuilder.selectIdPCallback = function (providers) {
  __rhinoLocalExpectArity("callbacksBuilder.selectIdPCallback", arguments, 1);
  __rhinoLocalCallback("SelectIdPCallback", { providers: providers, value: "" });
};

callbacksBuilder.termsAndConditionsCallback = function (version, terms, createDate) {
  __rhinoLocalExpectArity(
    "callbacksBuilder.termsAndConditionsCallback",
    arguments,
    3
  );
  __rhinoLocalCallback("TermsAndConditionsCallback", {
    version: String(version),
    terms: String(terms),
    createDate: String(createDate),
  });
};

callbacksBuilder.metadataCallback = function (outputValue) {
  __rhinoLocalExpectArity("callbacksBuilder.metadataCallback", arguments, 1);
  __rhinoLocalCallback("MetadataCallback", { data: outputValue });
};

callbacksBuilder.pollingWaitCallback = function (waitTime, message) {
  __rhinoLocalExpectArity("callbacksBuilder.pollingWaitCallback", arguments, 2);
  __rhinoLocalCallback("PollingWaitCallback", {
    waitTime: String(waitTime),
    message: String(message),
  });
};

callbacksBuilder.redirectCallback = function (
  redirectUrl,
  redirectData,
  method,
  fourth,
  fifth,
  sixth
) {
  var a = arguments;
  var redirectFields = {
    redirectUrl: String(redirectUrl),
    redirectData: redirectData,
    redirectMethod: String(method),
    // Follows the argument; the status parameter and redirect-back cookie are
    // not rendered (both measured).
    trackingCookie:
      a.length === 4 ? Boolean(fourth) : a.length === 6 ? Boolean(sixth) : false,
  };
  if (a.length >= 3 && a.length <= 6) {
    __rhinoLocalCallback("RedirectCallback", redirectFields);
    return;
  }
  throw __rhinoLocalArityRefusal("callbacksBuilder.redirectCallback", a, "3..6");
};

function __rhinoLocalAttributeCallback(type, args) {
  var fields = {
    name: String(args[0]),
    prompt: String(args[1]),
    value: args[2],
    required: Boolean(args[3]),
    policies: [],
    failedPolicies: [],
    validateOnly: false,
  };
  if (args.length === 4) {
    __rhinoLocalCallback(type, fields);
    return;
  }
  if (args.length === 5) {
    fields.failedPolicies = args[4];
    __rhinoLocalCallback(type, fields);
    return;
  }
  if (args.length === 6) {
    fields.policies = args[4];
    fields.validateOnly = Boolean(args[5]);
    __rhinoLocalCallback(type, fields);
    return;
  }
  if (args.length === 7) {
    fields.policies = args[4];
    fields.validateOnly = Boolean(args[5]);
    fields.failedPolicies = args[6];
    __rhinoLocalCallback(type, fields);
    return;
  }
  throw __rhinoLocalArityRefusal("callbacksBuilder." + type.charAt(0).toLowerCase() + type.substring(1), args, "4..7");
}

callbacksBuilder.stringAttributeInputCallback = function () {
  __rhinoLocalAttributeCallback("StringAttributeInputCallback", arguments);
};

callbacksBuilder.numberAttributeInputCallback = function () {
  __rhinoLocalAttributeCallback("NumberAttributeInputCallback", arguments);
};

callbacksBuilder.booleanAttributeInputCallback = function () {
  __rhinoLocalAttributeCallback("BooleanAttributeInputCallback", arguments);
};

callbacksBuilder.validatedUsernameCallback = function (
  prompt,
  policies,
  validateOnly,
  failedPolicies
) {
  if (arguments.length !== 3 && arguments.length !== 4) {
    throw __rhinoLocalArityRefusal("callbacksBuilder.validatedUsernameCallback", arguments, "3 or 4");
  }
  var userFields = {
    prompt: String(prompt),
    policies: policies,
    validateOnly: Boolean(validateOnly),
    failedPolicies: [],
  };
  if (arguments.length === 4) {
    userFields.failedPolicies = failedPolicies;
  }
  __rhinoLocalCallback("ValidatedCreateUsernameCallback", userFields);
};

callbacksBuilder.validatedPasswordCallback = function (
  prompt,
  echoOn,
  policies,
  validateOnly,
  failedPolicies
) {
  if (arguments.length !== 4 && arguments.length !== 5) {
    throw __rhinoLocalArityRefusal("callbacksBuilder.validatedPasswordCallback", arguments, "4 or 5");
  }
  var pwFields = {
    prompt: String(prompt),
    echoOn: Boolean(echoOn),
    policies: policies,
    validateOnly: Boolean(validateOnly),
    failedPolicies: [],
  };
  if (arguments.length === 5) {
    pwFields.failedPolicies = failedPolicies;
  }
  __rhinoLocalCallback("ValidatedCreatePasswordCallback", pwFields);
};

function __rhinoLocalRequireSubmitted() {
  if (__rhinoLocal.submittedCallbacks === null) {
    throw __rhinoLocalFixtureFault("callbacks", "rhino-local: callbacks: no given.callbacks");
  }
  return __rhinoLocal.submittedCallbacks;
}

// A Java List as AIC's getters return it: indexable, size/get/isEmpty/contains, and
// printed "[a, b]" with each element formatted by format().
function __rhinoLocalCallbackList(items, format) {
  var list = {};
  var i;
  for (i = 0; i < items.length; i += 1) {
    list[i] = items[i];
  }
  Object.defineProperty(list, "length", {
    enumerable: false,
    configurable: true,
    writable: true,
    value: items.length
  });
  __rhinoLocalHide(list, "size", function () {
    return items.length;
  });
  __rhinoLocalHide(list, "get", function (index) {
    return items[index] === undefined ? null : items[index];
  });
  __rhinoLocalHide(list, "isEmpty", function () {
    return items.length === 0;
  });
  __rhinoLocalHide(list, "contains", function (value) {
    return items.indexOf(value) !== -1;
  });
  __rhinoLocalHide(list, "toArray", function () {
    return __rhinoLocalArrayLike(items);
  });
  __rhinoLocalHide(list, "toString", function () {
    var values = [];
    var j;
    for (j = 0; j < items.length; j += 1) {
      values.push(format(items[j]));
    }
    return "[" + values.join(", ") + "]";
  });
  return list;
}

function __rhinoLocalCallbackValueString(value) {
  if (Array.isArray(value)) {
    return "[" + value.map(__rhinoLocalCallbackValueString).join(", ") + "]";
  }
  if (value === null || value === undefined) {
    return "null";
  }
  return String(value);
}

function __rhinoLocalCallbackMap(values) {
  var map = {};
  var keys = Object.keys(values);
  var i;
  for (i = 0; i < keys.length; i += 1) {
    map[keys[i]] = values[keys[i]];
  }
  __rhinoLocalHide(map, "size", function () {
    return keys.length;
  });
  __rhinoLocalHide(map, "get", function (key) {
    return __rhinoLocalHas(map, String(key)) ? map[String(key)] : null;
  });
  __rhinoLocalHide(map, "toString", function () {
    return __rhinoLocalJavaMapString(map);
  });
  __rhinoLocalHide(map, "__javaString", function () {
    var pairs = [];
    var j;
    for (j = 0; j < keys.length; j += 1) {
      pairs.push(keys[j] + "=" + __rhinoLocalCallbackValueString(map[keys[j]]));
    }
    return "{" + pairs.join(", ") + "}";
  });
  return map;
}

// A List of Java maps prints each as "{k=v, …}" (Map.toString).
function __rhinoLocalCallbackMapList(maps) {
  return __rhinoLocalCallbackList(maps, function (m) {
    return m.__javaString();
  });
}

function __rhinoLocalSubmittedValues(type) {
  var submitted = __rhinoLocalRequireSubmitted();
  var out = [];
  var i;
  var cb;
  for (i = 0; i < submitted.length; i += 1) {
    cb = submitted[i];
    if (cb.type === type) {
      if (cb.value === undefined) {
        throw __rhinoLocalFixtureFault("callback-value", 
          "rhino-local: given.callbacks[" +
            i +
            "] (" +
            type +
            ") has no value"
        );
      }
      out.push(cb.value);
    }
  }
  // Number values are Doubles on AIC: [7.0].
  var decimal = type === "NumberAttributeInputCallback";
  return __rhinoLocalCallbackList(out, function (value) {
    return decimal && typeof value === "number" && Math.floor(value) === value
      ? String(value) + ".0"
      : String(value);
  });
}

callbacks.isEmpty = function () {
  __rhinoLocalExpectArity("callbacks.isEmpty", arguments, 0);
  return __rhinoLocalRequireSubmitted().length === 0;
};

callbacks.getNameCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getNameCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("NameCallback");
};

callbacks.getPasswordCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getPasswordCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("PasswordCallback");
};

callbacks.getHiddenValueCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getHiddenValueCallbacks", arguments, 0);
  var submitted = __rhinoLocalRequireSubmitted();
  var values = {};
  var i;
  for (i = 0; i < submitted.length; i += 1) {
    if (submitted[i].type === "HiddenValueCallback") {
      values[String(submitted[i].id)] = submitted[i].value;
    }
  }
  return __rhinoLocalCallbackMap(values);
};

callbacks.getDeviceProfileCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getDeviceProfileCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("DeviceProfileCallback");
};

callbacks.getKbaCreateCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getKbaCreateCallbacks", arguments, 0);
  var submitted = __rhinoLocalRequireSubmitted();
  var maps = [];
  var i;
  for (i = 0; i < submitted.length; i += 1) {
    if (submitted[i].type === "KbaCreateCallback") {
      maps.push(__rhinoLocalCallbackMap({
        selectedAnswer: submitted[i].selectedAnswer,
        selectedQuestion: submitted[i].selectedQuestion,
      }));
    }
  }
  return __rhinoLocalCallbackMapList(maps);
};

callbacks.getSelectIdPCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getSelectIdPCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("SelectIdPCallback");
};

callbacks.getTermsAndConditionsCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getTermsAndConditionsCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("TermsAndConditionsCallback");
};

callbacks.getTextInputCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getTextInputCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("TextInputCallback");
};

callbacks.getStringAttributeInputCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getStringAttributeInputCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("StringAttributeInputCallback");
};

callbacks.getNumberAttributeInputCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getNumberAttributeInputCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("NumberAttributeInputCallback");
};

callbacks.getBooleanAttributeInputCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getBooleanAttributeInputCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("BooleanAttributeInputCallback");
};

callbacks.getConfirmationCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getConfirmationCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("ConfirmationCallback");
};

callbacks.getLanguageCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getLanguageCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("LanguageCallback");
};

callbacks.getIdpCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getIdpCallbacks", arguments, 0);
  var submitted = __rhinoLocalRequireSubmitted();
  var maps = [];
  var i;
  for (i = 0; i < submitted.length; i += 1) {
    if (submitted[i].type === "IdPCallback") {
      // nodeName, userInfo and requestNativeAppForUserInfo are not in the
      // rendered callback; these are the values AIC's map held (measured).
      maps.push(__rhinoLocalCallbackMap({
        nodeName: "IdPCallback",
        redirectUri: submitted[i].redirectUri,
        request: submitted[i].request,
        acrValues: submitted[i].acrValues,
        userInfo: null,
        clientId: submitted[i].clientId,
        requestNativeAppForUserInfo: false,
        requestUri: submitted[i].requestUri,
        nonce: submitted[i].nonce,
        token: submitted[i].token,
        provider: submitted[i].provider,
        scope: submitted[i].scopes,
        tokenType: submitted[i].tokenType,
      }));
    }
  }
  return __rhinoLocalCallbackMapList(maps);
};

callbacks.getValidatedPasswordCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getValidatedPasswordCallbacks",
    arguments,
    0
  );
  return __rhinoLocalValidatedCallbacks("ValidatedCreatePasswordCallback");
};

callbacks.getValidatedUsernameCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getValidatedUsernameCallbacks",
    arguments,
    0
  );
  return __rhinoLocalValidatedCallbacks("ValidatedCreateUsernameCallback");
};

function __rhinoLocalValidatedCallbacks(type) {
  var submitted = __rhinoLocalRequireSubmitted();
  var maps = [];
  var i;
  for (i = 0; i < submitted.length; i += 1) {
    if (submitted[i].type === type) {
      maps.push(__rhinoLocalCallbackMap({
        validateOnly: submitted[i].validateOnly,
        value: submitted[i].value,
      }));
    }
  }
  return __rhinoLocalCallbackMapList(maps);
}

callbacks.getHttpCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getHttpCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("HttpCallback");
};

callbacks.getX509CertificateCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getX509CertificateCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("X509CertificateCallback");
};

callbacks.getConsentMappingCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getConsentMappingCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("ConsentMappingCallback");
};

callbacks.getChoiceCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getChoiceCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("ChoiceCallback");
};

function __rhinoLocalSplitResource(id) {
  var s = String(id);
  var parts = s.split("/");
  if (parts.length >= 3) {
    return {
      collection: parts.slice(0, parts.length - 1).join("/"),
      recordId: parts[parts.length - 1],
    };
  }
  return { collection: s, recordId: null };
}

function __rhinoLocalFindRecord(collection, recordId) {
  var rows = __rhinoLocal.managed[collection];
  if (!rows) {
    return { rows: null, index: -1, record: null };
  }
  var i;
  for (i = 0; i < rows.length; i += 1) {
    if (String(rows[i]._id) === String(recordId)) {
      return { rows: rows, index: i, record: rows[i] };
    }
  }
  return { rows: rows, index: -1, record: null };
}

// A bare managed-object type collection, `managed/<type>`. A profile speaks
// for these and nothing else: it lists managed types, so a relationship
// collection (`managed/<type>/<id>/roles`), `internal/…`, `config/…` or
// `system/…` is neither declared nor absent by it, and stays a missing fixture
// exactly as without a profile (round-3 P1-1).
function __rhinoLocalIsManagedType(collection) {
  return /^managed\/[^/]+$/.test(String(collection));
}

// Object types this environment declares, from a pulled harness profile.
// `null` means no profile was supplied, which keeps the pre-profile behaviour:
// an unseeded collection is always a missing fixture.
function __rhinoLocalDeclaresType(collection) {
  if (!__rhinoLocal.knownTypes || !__rhinoLocalIsManagedType(collection)) {
    return false;
  }
  return __rhinoLocalHas(__rhinoLocal.knownTypes, collection);
}

// A write to a type the profile says the tenant does not have. Only read
// (null) and query (Resource … not found) are measured; for create, update,
// patch, delete and action it is unmeasured, so the harness cannot answer.
function __rhinoLocalUnknownTypeFault(method, collection) {
  return __rhinoLocalFault(
    "openidm-unknown-managed-type",
    "rhino-local: openidm." +
      method +
      ": " +
      JSON.stringify(collection) +
      " is not a managed object in environment " +
      JSON.stringify(__rhinoLocal.profileTenant) +
      " (pulled " +
      String(__rhinoLocal.profilePulledAt) +
      "), and what AIC does for " +
      method +
      " on an absent type is unmeasured. Check the name, or re-pull the profile if the tenant has changed."
  );
}

function __rhinoLocalRequireCollection(method, collection) {
  if (__rhinoLocalHas(__rhinoLocal.managed, collection)) {
    return __rhinoLocal.managed[collection];
  }
  // With a profile, a declared-but-unseeded type is an EMPTY collection, which
  // is what AIC presents: read returns null rather than failing. Without one we
  // cannot tell that from a typo, so the miss stays a missing-fixture throw.
  if (__rhinoLocalDeclaresType(collection)) {
    __rhinoLocal.managed[collection] = [];
    return __rhinoLocal.managed[collection];
  }
  // With a profile the type is known to be absent from the tenant. read and
  // query emulate AIC (null, and a catchable not-found) before reaching here;
  // what AIC does for update, patch or delete on an absent type is unmeasured.
  if (__rhinoLocal.knownTypes && __rhinoLocalIsManagedType(collection)) {
    throw __rhinoLocalUnknownTypeFault(method, collection);
  }
  throw __rhinoLocalFixtureFault("managed",
    "rhino-local: openidm." +
      method +
      ": no given.managed entry for " +
      JSON.stringify(collection) +
      "; seed it, or run with a profile so a type absent from the tenant behaves as on AIC"
  );
}

// A write that reaches this lane's store on a type whose profile declares hooks
// or policies (E15b). A write a test stubs with given.openidmFailures has
// already thrown in __rhinoLocalPushOpenidm and never gets here.
function __rhinoLocalPolicyRule(collection) {
  var table = __rhinoLocal.managedPolicy;
  return table && __rhinoLocalHas(table, collection) ? table[collection] : null;
}

function __rhinoLocalJsonType(value) {
  if (value === null) {
    return "null";
  }
  return Array.isArray(value) ? "array" : typeof value;
}

function __rhinoLocalTypeMatches(value, declared) {
  var actual = __rhinoLocalJsonType(value);
  // No JSON number is an `integer` to IDM's type check (measured 2026-11-08, docs/api/10).
  return actual === declared;
}

// IDM's implicit schema policy on the top-level properties a write sends
// (P2-2, sol-r1b; the rule is write-guard.ts managedPolicy). Measured on a
// create of a new record (docs/api/10, the 2026-11-08 probe over REST and from
// an IDM script; docs/api/12 for this next-gen openidm.create's surface):
//   - a null on a defaulted property is NOT_NULL; a null on any typed
//     property is VALID_TYPE when the schema has no required array; any other
//     null is accepted and stored;
//   - a value whose JSON type is not the declared string/object/array/
//     boolean/number/integer is VALID_TYPE (every number is "number", so an
//     integer property refuses all of them); nothing nested under
//     items/properties is checked;
//   - a null on an array declaring default: null is stored as [] (emptyOnNull).
// Any of those refuses the write: 403 "Policy validation failed", which this
// lane throws as the adapter exception the measured NOT_NULL showed. Which
// requirements getDetail() lists is the policy's answer, but its Java type and
// string form here are unmeasured, so reading it faults. Everything else that
// policy would judge is unmeasured and faults: a top-level array item of the
// wrong items type (only nested items were measured), a null on a nullable
// property, a non-null value of the wrong type on a nullable property whose
// spelling was never probed (mismatchUnmeasured, sol-r3 P2-3), a null on an emptyOnNull array outside a new record's create, and
// any refusal or
// defaulted null outside a new record's create. A null on an undefaulted
// property outside a create, where a create would accept it, writes as on
// create (assumed).
function __rhinoLocalImplicitPolicy(method, collection, content, creatingNew) {
  var rule = __rhinoLocalPolicyRule(collection);
  if (!rule || content === null || typeof content !== "object") {
    return;
  }
  var type = collection.replace(/^managed\//, "");
  var refused = [];
  var unmeasured = [];
  Object.keys(content).forEach(function (name) {
    var value = content[name];
    if (value === null) {
      if (rule.nullUnmeasured.indexOf(name) >= 0) {
        unmeasured.push(name + " is null on a nullable or untyped property, which nothing measured");
      } else if (rule.emptyOnNull.indexOf(name) >= 0 && !(method === "create" && creatingNew)) {
        unmeasured.push(name + " is null on an array declaring default: null; AIC stores [] for that on a create of a " +
          "new record, and this write is not one");
      } else if (rule.nullRefused.indexOf(name) >= 0) {
        refused.push(name + (rule.notNull.indexOf(name) >= 0 ? " (NOT_NULL: it declares a non-null default)" : " (VALID_TYPE: the schema has no required array)"));
      }
      return;
    }
    if (__rhinoLocalHas(rule.checked, name) && !__rhinoLocalTypeMatches(value, rule.checked[name])) {
      refused.push(name + " (VALID_TYPE: declared " + rule.checked[name] + ", sent " + __rhinoLocalJsonType(value) + ")");
    } else if (__rhinoLocalHas(rule.mismatchUnmeasured, name) && !__rhinoLocalTypeMatches(value, rule.mismatchUnmeasured[name])) {
      unmeasured.push(name + " is " + __rhinoLocalJsonType(value) + " on a nullable " + rule.mismatchUnmeasured[name] +
        " property; the plain type's VALID_TYPE refusal was measured, this nullable spelling's never was");
    } else if (Array.isArray(value) && __rhinoLocalHas(rule.itemTypes, name)) {
      value.forEach(function (item, index) {
        if (!__rhinoLocalTypeMatches(item, rule.itemTypes[name])) {
          unmeasured.push(name + "[" + index + "] is " + __rhinoLocalJsonType(item) + " and items are declared " + rule.itemTypes[name] +
            "; nested items schemas were measured unenforced, but a top-level array's own items were not tested");
        }
      });
    }
  });
  if (unmeasured.length > 0) {
    throw __rhinoLocalFault("openidm-managed-policy/" + type,
      "rhino-local: openidm." + method + ": " + collection + ": " + unmeasured.join("; ") + " (docs/api/10). Stub the call " +
        "with given.openidmFailures if the test means a refusal, or expect the fault (E19)");
  }
  if (refused.length === 0) {
    return;
  }
  if (method === "create" && creatingNew) {
    var error = __rhinoLocalAdapterException("Policy validation failed", 403);
    __rhinoLocalHide(error.javaException, "getDetail", function () {
      throw __rhinoLocalFault("openidm-policy-detail",
        "rhino-local: javaException.getDetail(): on AIC it lists the failed policy requirements for " + refused.join(", ") +
          " (docs/api/10, 12), but its Java type and string form on this path are unmeasured");
    });
    throw error;
  }
  throw __rhinoLocalFault("openidm-managed-policy/" + type,
    "rhino-local: openidm." + method + ": " + collection + ": the write fails policy on " + refused.join(", ") +
      ". AIC's 403 for that is measured only on a create of a new record (docs/api/10, 12); this write is not one, so " +
      "what AIC answers is unmeasured. Omit a defaulted key to let the default land, or stub the call with " +
      "given.openidmFailures if the test means a refusal");
}

function __rhinoLocalApplyDefaults(collection, stored) {
  var rule = __rhinoLocalPolicyRule(collection);
  if (!rule) {
    return;
  }
  Object.keys(rule.defaults).forEach(function (name) {
    if (__rhinoLocalHas(stored, name)) {
      return;
    }
    // A null default is measured applied (stored as null, [] on an array) on
    // every non-nullable type when the schema has a required array, and on a
    // string without one; the rest are in nullUnmeasured.
    if (rule.defaults[name] === null && rule.nullUnmeasured.indexOf(name) >= 0) {
      throw __rhinoLocalFault("openidm-managed-policy/" + collection.replace(/^managed\//, ""),
        "rhino-local: openidm.create: " + collection + ": " + name + " declares default: null on a nullable type, or " +
          "on a non-string type in a schema with no required array. A null default was measured applied on every plain " +
          "type with a required array, and on a string without one (docs/api/10); this case was not, so what AIC " +
          "stores is unmeasured. Send the property, or expect the fault (E19)");
    }
    // defineProperty, not assignment: the name is the schema's, and a property
    // named __proto__ would otherwise hit the prototype setter (astra-r1 P2-2).
    Object.defineProperty(stored, name, {
      value: __rhinoLocalClone(rule.defaults[name]),
      enumerable: true,
      writable: true,
      configurable: true
    });
  });
  // An explicit null on an array declaring default: null is stored as [] too
  // (measured 2026-11-08, docs/api/10); only a new record's create gets here.
  rule.emptyOnNull.forEach(function (name) {
    if (__rhinoLocalHas(stored, name) && stored[name] === null) {
      Object.defineProperty(stored, name, { value: [], enumerable: true, writable: true, configurable: true });
    }
  });
}

// A PATCH `remove` of a property that declares a `default`, null included
// (P3-1). AIC's 403 REQUIRED was measured only with the property also named
// in schema.required, which the lane already faults as a declared policy.
// With the property not required the answer is unmeasured, so neither a 403
// nor a 200 is emulated.
function __rhinoLocalRemovedDefault(collection, patch) {
  var rule = __rhinoLocalPolicyRule(collection);
  if (!rule) {
    return;
  }
  var i;
  var field;
  for (i = 0; i < patch.length; i += 1) {
    field = String(patch[i].field).replace(/^\//, "");
    if (patch[i].operation === "remove" && rule.defaulted.indexOf(field) >= 0) {
      throw __rhinoLocalFault("openidm-managed-policy/" + collection.replace(/^managed\//, ""),
        "rhino-local: openidm.patch: " + collection + ": remove " + String(patch[i].field) + ", which declares a default. " +
          "AIC's 403 REQUIRED for that is measured only with the property also named in schema.required " +
          "(docs/api/10); without it, what AIC answers is unmeasured. Stub the call with given.openidmFailures if the test means a refusal, or expect the fault (E19)");
    }
  }
}

function __rhinoLocalManagedWriteFault(method, collection) {
  var table = __rhinoLocal.managedWriteFaults;
  if (!table || !__rhinoLocalHas(table, collection) || !__rhinoLocalHas(table[collection], method)) {
    return;
  }
  var fault = table[collection][method];
  throw __rhinoLocalFault(fault.id, "rhino-local: openidm." + method + ": " + fault.message);
}

// A profile with an invalid default anywhere (sol-r1c): AIC stores that
// config/managed but keeps serving its last good compile for every type
// (measured 2026-11-08, docs/api/10), which the profile does not describe. So
// every call on any managed/<type> faults, before anything else is judged,
// decided in write-guard.ts managedInvalidDefault.
function __rhinoLocalInvalidDefault(method, collection) {
  var fault = __rhinoLocal.managedInvalidDefault;
  if (!fault || !/^managed\/[^/]+/.test(String(collection))) {
    return;
  }
  // The message starts with the offending type ("managed/gizmo: ..."); name the
  // called collection only when it is another type, so the prefix is not repeated.
  var own = fault.message.indexOf(collection + ": ") === 0;
  throw __rhinoLocalFault(fault.id, "rhino-local: openidm." + method + ": " +
    (own ? "" : collection + ": faulted for another type's declaration: ") + fault.message);
}

// A type a profile says the tenant does not have. Without a profile an
// unseeded collection cannot be told from a typo, so it is never absent.
function __rhinoLocalAbsentType(collection) {
  return (
    !__rhinoLocalHas(__rhinoLocal.managed, collection) &&
    !!__rhinoLocal.knownTypes &&
    __rhinoLocalIsManagedType(collection) &&
    !__rhinoLocalDeclaresType(collection)
  );
}

function __rhinoLocalRequireRecord(method, resource) {
  var split = __rhinoLocalSplitResource(resource);
  if (split.recordId === null) {
    throw __rhinoLocalFault("openidm-collection-path-as-record", 
      "rhino-local: openidm." +
        method +
        ": " +
        JSON.stringify(resource) +
        " is a collection path, not a record"
    );
  }
  __rhinoLocalRequireCollection(method, split.collection);
  var found = __rhinoLocalFindRecord(split.collection, split.recordId);
  if (!found.record) {
    throw __rhinoLocalEmulated(
      "rhino-local: openidm." + method + ": No Such Entry: " + resource
    );
  }
  return found;
}

// Where the script called in from: the first frame of Rhino's
// `\tat <file>:<line>` stack outside this runtime's own source. Null where
// the stack has another shape (the in-process vm lane) or no such frame.
function __rhinoLocalCallSite() {
  var stack;
  try {
    stack = String(new Error("call site").stack);
  } catch (e) {
    return null;
  }
  var frames = [];
  stack.split("\n").forEach(function (line) {
    var m = /^\tat (.+?):(\d+)(?: \(.*\))?\s*$/.exec(line);
    if (m) {
      frames.push({ file: m[1], line: m[2] });
    }
  });
  var i;
  for (i = 1; i < frames.length; i += 1) {
    if (frames[i].file !== frames[0].file) {
      return frames[i].file + ":" + frames[i].line;
    }
  }
  return null;
}

// What a `fields` parent is, with its article, for fault text.
function __rhinoLocalKind(value) {
  if (value === null) {
    return "null";
  }
  if (Array.isArray(value)) {
    return __rhinoLocalIsReferenceArray(value) ? "a relationship array" : "an array of non-references";
  }
  return (typeof value === "object" ? "an " : "a ") + typeof value;
}

// One fault shape for every unmeasured `fields` projection: the operation,
// the resource, the selector, what the parent was and the script's call site.
function __rhinoLocalFieldsFault(id, context, selector, detail, remedy) {
  var site = __rhinoLocalCallSite();
  return __rhinoLocalFault(id,
    "rhino-local: openidm." + context.method + " " + context.resource + " fields " + JSON.stringify(selector) +
      ": " + detail + "; " + remedy + (site ? " (called from " + site + ")" : ""));
}

// A has-many relationship as a seed holds it: an array whose every element is
// an object carrying a `_ref` string. An empty array is one too: AIC answers
// `[]` for every selector on an empty relationship (measured 2026-11-08).
function __rhinoLocalIsReferenceArray(value) {
  return Array.isArray(value) && value.every(function (el) {
    return __rhinoLocalIsPlainObject(el) && typeof el._ref === "string";
  });
}

function __rhinoLocalProject(record, fields, context) {
  if (!fields || !Array.isArray(fields) || fields.length === 0) {
    return record;
  }
  context = context || { method: "read", resource: "?" };
  // AM splits every selector element on commas without trimming whitespace.
  fields = fields.reduce(function (names, entry) {
    return names.concat(String(entry).split(","));
  }, []);
  var wildcard = fields.indexOf("*") !== -1;
  // Every child selector is judged before anything is returned, so neither a
  // wildcard nor a whole-parent selector hides an unmeasured one.
  // Keyed by caller-chosen names, so no prototype: a `__proto__` parent is data.
  var parents = [];
  var byParent = Object.create(null);
  fields.forEach(function (field) {
    if (field.indexOf("/") === -1) {
      return;
    }
    var parent = field.split("/")[0];
    if (!byParent[parent]) {
      byParent[parent] = [];
      parents.push(parent);
    }
    byParent[parent].push(field);
  });
  var projected = Object.create(null);
  parents.forEach(function (parent) {
    if (!__rhinoLocalHas(record, parent)) {
      return;
    }
    var value = record[parent];
    if (__rhinoLocalIsReferenceArray(value)) {
      var selectors = byParent[parent].slice();
      if (fields.indexOf(parent) !== -1) {
        selectors.push(parent);
      }
      projected[parent] = __rhinoLocalOpenidmList(__rhinoLocalProjectReferences(value, selectors, context));
      return;
    }
    if (wildcard) {
      throw __rhinoLocalFieldsFault("openidm-fields-wildcard-with-path", context, byParent[parent][0],
        "combining * with a child selector is unmeasured unless the parent is a relationship array, and " + parent + " is " + __rhinoLocalKind(value),
        "use separate selectors without a wildcard");
    }
    var out;
    byParent[parent].forEach(function (field) {
      out = __rhinoLocalProjectChild(value, field.split("/").slice(1), out, context, field);
    });
    if (fields.indexOf(parent) === -1) {
      projected[parent] = out;
    }
  });
  var result;
  var i;
  if (wildcard) {
    result = {};
    // A relationship named on its own beside `*` is selected as it is
    // without `*`: the same projected list.
    Object.keys(record).forEach(function (key) {
      __rhinoLocalSetOwn(result, key, fields.indexOf(key) !== -1 && __rhinoLocalIsReferenceArray(record[key])
        ? __rhinoLocalOpenidmList(__rhinoLocalProjectReferences(record[key], [key], context))
        : record[key]);
    });
  } else {
    result = {};
    if (record._id !== undefined) {
      result._id = record._id;
    }
    if (record._rev !== undefined) {
      result._rev = record._rev;
    }
    for (i = 0; i < fields.length; i += 1) {
      if (__rhinoLocalHas(record, fields[i])) {
        __rhinoLocalSetOwn(result, fields[i], __rhinoLocalIsReferenceArray(record[fields[i]])
          ? __rhinoLocalOpenidmList(__rhinoLocalProjectReferences(record[fields[i]], [fields[i]], context))
          : record[fields[i]]);
      }
    }
  }
  Object.keys(projected).forEach(function (parent) {
    __rhinoLocalSetOwn(result, parent, projected[parent]);
  });
  return result;
}

// Only the reference envelope documented with _meta/lastChanged is measured.
var __rhinoLocalReferenceEnvelope = [
  "_id", "_rev", "_ref", "_refResourceCollection", "_refResourceId", "_refResourceRev", "_refProperties"
];
// What a bare has-many selector returns per element (measured 2026-11-08).
var __rhinoLocalReferenceBare = ["_ref", "_refResourceCollection", "_refResourceId", "_refProperties"];

// One element's reference envelope. `_refResourceCollection` and
// `_refResourceId` are spellings of `_ref`, so they are derived when the seed
// leaves them out; `_id` is the target's id, and `_rev` / `_refResourceRev`
// are both the target's revision, so either fills the other. Nothing else is
// invented: a seed without a revision reads without one.
function __rhinoLocalEnvelopeOf(el, keys) {
  var ref = String(el._ref);
  var slash = ref.lastIndexOf("/");
  var derived = {
    _ref: ref,
    _refResourceCollection: __rhinoLocalHas(el, "_refResourceCollection") ? el._refResourceCollection : ref.substring(0, slash),
    _refResourceId: __rhinoLocalHas(el, "_refResourceId") ? el._refResourceId : ref.substring(slash + 1),
  };
  derived._id = __rhinoLocalHas(el, "_id") ? el._id : derived._refResourceId;
  if (__rhinoLocalHas(el, "_rev") || __rhinoLocalHas(el, "_refResourceRev")) {
    derived._rev = __rhinoLocalHas(el, "_rev") ? el._rev : el._refResourceRev;
    derived._refResourceRev = __rhinoLocalHas(el, "_refResourceRev") ? el._refResourceRev : el._rev;
  }
  if (__rhinoLocalHas(el, "_refProperties")) {
    derived._refProperties = el._refProperties;
  }
  var out = {};
  keys.forEach(function (key) {
    if (__rhinoLocalHas(derived, key)) {
      out[key] = derived[key];
    }
  });
  return out;
}

// An element carries its target's fields only if the seed put some there.
// One holding nothing past the envelope is a bare reference, and expanding it
// from another seeded collection is unmeasured.
function __rhinoLocalRequireExpanded(el, index, parent, selector, context) {
  var expanded = Object.keys(el).some(function (key) {
    return __rhinoLocalReferenceEnvelope.indexOf(key) === -1;
  });
  if (!expanded) {
    throw __rhinoLocalFieldsFault("openidm-fields-unresolved-ref", context, selector,
      "element " + index + " of " + parent + " (" + String(el._ref) + ") is a bare reference, and reference expansion is unmeasured",
      "seed that element pre-expanded with the target's fields alongside its _ref");
  }
}

// A has-many relationship under the selectors that name it. Measured
// 2026-11-08 on a two-element alpha_user.roles (docs/api/12): the bare name
// gives the reference envelope; `parent/*` adds `_id`, `_rev`,
// `_refResourceRev` and every target field; `parent/*/<f>` (any deeper
// suffix, when <f> is not an object) adds those three and <f>; any other
// `parent/<x>` adds only the three, never <x>. Several selectors give their
// union. AIC's element order varied between reads; seeded order is kept.
function __rhinoLocalProjectReferences(value, selectors, context) {
  var parent = selectors[0].split("/")[0];
  var s;
  var path;
  // Checked once, before any element and so on an empty array too: only one
  // segment past a non-`*` name, and nothing below an envelope key, was
  // measured. These depend on the selector alone, so a deeper path refuses
  // whatever the relationship holds, even where the shallow form returns the
  // envelope.
  for (s = 0; s < selectors.length; s += 1) {
    path = selectors[s].split("/").slice(1);
    if (path.length > 1 && path[0] !== "*") {
      throw __rhinoLocalFieldsFault("openidm-fields-relationship-deep-path", context, selectors[s],
        "a path below " + parent + "/" + path[0] + " is unmeasured (only " + parent + "/<name> and " + parent + "/*/... were measured)",
        "select " + parent + "/" + path[0] + " or " + parent + "/*");
    }
    if (path.length > 2 && path[0] === "*" && __rhinoLocalReferenceEnvelope.indexOf(path[1]) !== -1) {
      throw __rhinoLocalFieldsFault("openidm-fields-relationship-deep-path", context, selectors[s],
        "a path below the reference envelope key " + path[1] + " is unmeasured",
        "select " + parent + "/*/" + path[1] + " and read the nested value in the script");
    }
  }
  return value.map(function (el, index) {
    var out = __rhinoLocalEnvelopeOf(el, __rhinoLocalReferenceBare);
    selectors.forEach(function (selector) {
      var path = selector.split("/").slice(1);
      if (path.length === 0) {
        return;
      }
      var system = __rhinoLocalEnvelopeOf(el, __rhinoLocalReferenceEnvelope);
      Object.keys(system).forEach(function (key) {
        out[key] = system[key];
      });
      if (path[0] !== "*") {
        return;
      }
      __rhinoLocalRequireExpanded(el, index, parent, selector, context);
      if (path.length === 1) {
        Object.keys(el).forEach(function (key) {
          if (!__rhinoLocalHas(out, key)) {
            __rhinoLocalSetOwn(out, key, el[key]);
          }
        });
        return;
      }
      var field = path[1];
      if (!__rhinoLocalHas(el, field) || __rhinoLocalHas(system, field)) {
        return;
      }
      if (path.length > 2 && el[field] !== null && typeof el[field] === "object") {
        throw __rhinoLocalFieldsFault("openidm-fields-relationship-deep-path", context, selector,
          "a path below an object field of a relationship element is unmeasured (" + field + " is " + __rhinoLocalKind(el[field]) + ")",
          "select " + parent + "/*/" + field + " and read the nested value in the script");
      }
      __rhinoLocalSetOwn(out, field, el[field]);
    });
    return out;
  });
}

function __rhinoLocalProjectChild(value, path, previous, context, selector) {
  if (!__rhinoLocalIsPlainObject(value)) {
    throw __rhinoLocalFieldsFault("openidm-fields-non-object-parent", context, selector,
      "projecting a child of " + __rhinoLocalKind(value) + " is unmeasured (a single-reference object and a relationship array are)",
      Array.isArray(value)
        ? "seed each element as a reference object with a _ref, or select the whole property"
        : "select the whole property and read the child in the script");
  }
  var child = path[0];
  if (__rhinoLocalHas(value, "_ref") && !__rhinoLocalHas(value, child)) {
    throw __rhinoLocalFieldsFault("openidm-fields-unresolved-ref", context, selector,
      "the reference " + String(value._ref) + " carries no " + child + ", and reference expansion is unmeasured",
      "seed the requested child in a pre-expanded object parent");
  }
  var out = previous || {};
  __rhinoLocalReferenceEnvelope.forEach(function (key) {
    if (__rhinoLocalHas(value, key)) {
      out[key] = value[key];
    }
  });
  if (__rhinoLocalHas(value, child)) {
    // Only an own accumulated child is reused: `out.__proto__` would be Object.prototype.
    __rhinoLocalSetOwn(out, child, path.length === 1 ? value[child]
      : __rhinoLocalProjectChild(value[child], path.slice(1), __rhinoLocalHas(out, child) ? out[child] : undefined, context, selector));
  }
  return out;
}

function __rhinoLocalPushOpenidm(method, resource, body, actionName) {
  var rec = { method: method, resource: resource };
  if (body !== undefined) {
    rec.body =
      method === "create" ? __rhinoLocalCloneUndefinedAsNull(body, true) : __rhinoLocalClone(body);
  }
  if (actionName !== undefined) {
    rec.actionName = actionName;
  }
  __rhinoLocal.openidm.push(rec);
  var key = method + " " + resource;
  var ordinal = (__rhinoLocal.openidmCallCounts[key] || 0) + 1;
  __rhinoLocal.openidmCallCounts[key] = ordinal;
  var stubs = __rhinoLocal.openidmFailures;
  var i;
  var stub;
  for (i = 0; i < stubs.length; i += 1) {
    stub = stubs[i];
    if (stub.match.method !== method || stub.match.ordinal !== ordinal) {
      continue;
    }
    if (!__rhinoLocalMatchPattern(stub.match.resource, resource, "openidmFailures", "resource")) {
      continue;
    }
    // No `code` property: AIC's ResourceExceptionScriptAdapter has none
    // (live-openidm-error-shape.e2e.test.ts measured typeof error.code as
    // "undefined"). The code is in the message text and, as on every
    // adapter, behind e.javaException.getCode(); the stub's code is the
    // status this failure stands for.
    throw __rhinoLocalAdapterException(
      stub.reply.message !== undefined ? stub.reply.message : "injected ResourceException code " + stub.reply.code,
      stub.reply.code,
      Object.prototype.hasOwnProperty.call(stub.reply, "detail") ? { value: stub.reply.detail } : undefined
    );
  }
}

// A filter AIC rejects with its parse error: crest-filter.cjs answers
// "parse" only for inputs measured to be rejected (the script text is measured
// for "name eq", docs/api/12; the REST 400 for `not`, `eq null`, `in` and
// array values, docs/api/10) or rejected by any grammar CREST could have.
// Catchable, as on AIC.
function __rhinoLocalFilterError(filter) {
  throw __rhinoLocalEmulated(
    "rhino-local: openidm.query: The value '" + String(filter) +
      "' for parameter '_queryFilter' could not be parsed as a valid query filter"
  );
}

// Anything else the harness cannot decide: a filter the shared grammar cannot
// read (AIC may parse it), an operator this matcher does not evaluate
// (gt/ge/lt/le and any other word), and `ne`, which AIC refuses over REST with
// a measured 400 but whose script-side text is unmeasured. A fault, never a
// catchable error, so a script's catch branch cannot pass on a guess.
function __rhinoLocalFilterUnsupported(filter, reason) {
  throw __rhinoLocalFault("openidm-query-filter",
    "rhino-local: openidm.query: unmocked filter " + JSON.stringify(filter) +
      " (" + reason + ")"
  );
}

// update and patch with a stale non-null _rev (measured on both).
// The message is measured; the status this binding reports for it is not (a
// stale If-Match over REST was measured 412 on a different request), so it
// carries no e.javaException (review pf-02 P3-3).
function __rhinoLocalCheckRev(method, rev, record) {
  if (rev !== null && rev !== undefined && String(rev) !== String(record._rev)) {
    var error = __rhinoLocalJavaException(
      "org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: openidm." +
        method + ": The resource could not be accessed because the expected version '" +
        String(rev) +
        "' does not match the current version '" +
        String(record._rev) +
        "'"
    );
    throw error;
  }
}

function __rhinoLocalOpenidmRecord(value, id, rev) {
  var record = { _id: id, _rev: rev };
  var keys = Object.keys(value);
  var i;
  for (i = 0; i < keys.length; i += 1) {
    if (keys[i] !== "_id" && keys[i] !== "_rev") {
      // A key named __proto__ (a schema default's, or the script's) stays data (astra-r1 P2-2).
      Object.defineProperty(record, keys[i], { value: value[keys[i]], enumerable: true, writable: true, configurable: true });
    }
  }
  return record;
}

// The filter's grammar and evaluation are the shared ones
// (crest-filter.cjs), which the IDM emulator runs too. This lane has no
// per-field string policy, so any leaf whose answer moves under a case,
// space or Unicode reading is unknown (round-3 P3-1), as is a type mismatch
// nobody measured (round-4 P2-3), the quoted integer included (measured on
// alpha_user only, round-5 P2-1). A record whose whole answer is unknown is a
// fault. Runs before any row is read, as AIC parses before it reads.
function __rhinoLocalParseFilter(filter, lane) {
  var prepared = __rhinoLocalCrestFilter.prepare(filter, lane.admits);
  if (prepared.error === "parse") {
    __rhinoLocalFilterError(filter);
  }
  if (prepared.error === "ne") {
    __rhinoLocalFilterUnsupported(filter, "AIC refuses ne; its script-side text is unmeasured");
  }
  if (prepared.error) {
    __rhinoLocalFilterUnsupported(filter, prepared.reason);
  }
  return prepared.ast;
}

// AIC's 400 for a literal a DS integer or time attribute cannot convert, as
// AM's openidm.query surfaces it (second probe, 2026-11-06, docs/api/12):
// a catchable JavaException whose message is the adapter's class and the
// REST message, HTML escaping included (`&#39;`); no `(script#line)` suffix;
// `e.javaException` answers getCode() 400 and getReason() "Bad Request", and
// nothing else (getMessage and getClass are TypeErrors on AIC too). Only a
// letter-only or decimal literal reaches here, so `'` is the one character
// REST's error escaping can touch.
function __rhinoLocalIllegalAssertion(illegal) {
  return __rhinoLocalAdapterException(
    "The request cannot be processed because it contained an illegal filter assertion value &#39;" +
      illegal.value + "&#39; for field &#39;" + illegal.field + "&#39;", 400);
}

var __rhinoLocalUndecidedWhy = {
  "case": "turns on letter case",
  space: "turns on leading, trailing or repeated spaces or non-U+0020 whitespace",
  unicode: "turns on Unicode normalisation or characters RFC 4518 maps to nothing",
  type: "compares different JSON types, or an object or null element, which nothing measured",
  presence: "asks pr of an empty string, array or object, which nothing measured",
  rule: "is on a user field whose DS matching rule is not caseIgnoreMatch (telephone, time or integer), and the 2026-11-06 probes measured neither this field nor this operator and literal form",
  path: "reads through an array, measured only as a bare eq reached through and alone",
};

openidm.read = function (resourceName, _params, fields) {
  if (arguments.length < 1 || arguments.length > 3) {
    throw __rhinoLocalArityRefusal("openidm.read", arguments, "1..3");
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("read", resource);
  var split = __rhinoLocalSplitResource(resource);
  __rhinoLocalInvalidDefault("read", split.collection);
  if (split.recordId === null) {
    throw __rhinoLocalFault("openidm-collection-path-as-record", 
      "rhino-local: openidm.read: " +
        JSON.stringify(resource) +
        " is a collection path, not a record"
    );
  }
  // AIC returns null for a record of a type the tenant does not have
  // (docs/api/10, measured on managed/zzz_no_such_type/x).
  if (__rhinoLocalAbsentType(split.collection)) {
    return null;
  }
  __rhinoLocalRequireCollection("read", split.collection);
  var found = __rhinoLocalFindRecord(split.collection, split.recordId);
  if (!found.record) {
    // AIC returns null for a missing record in a known collection
    // (docs/api/10, verified 2026-07-17). An unseeded collection still
    // throws above — that is a missing given.managed fixture.
    return null;
  }
  __rhinoLocalBagAccess("read", resource, found.record, fields);
  return __rhinoLocalProject(found.record, fields, { method: "read", resource: resource });
};

openidm.query = function (resourceName, params, fields) {
  if (arguments.length < 2 || arguments.length > 3) {
    throw __rhinoLocalArityRefusal("openidm.query", arguments, "2 or 3");
  }
  var collection = String(resourceName);
  __rhinoLocalPushOpenidm("query", collection, params);
  __rhinoLocalInvalidDefault("query", collection);
  // AIC's text, HTML-escaped as it comes back (corpus binding-openidm-writes
  // query/2/absent-type, live; docs/api/12 openidm query).
  if (__rhinoLocalAbsentType(collection)) {
    throw __rhinoLocalEmulated(
      "rhino-local: openidm.query: Resource &#39;" + collection + "&#39; not found"
    );
  }
  var rows = __rhinoLocalRequireCollection("query", collection);
  var kinds = 0;
  var kindNames = ["_queryId", "_queryExpression", "_queryFilter"];
  var k;
  for (k = 0; k < kindNames.length; k += 1) {
    if (params && params[kindNames[k]] !== undefined) {
      kinds += 1;
    }
  }
  if (kinds !== 1) {
    throw __rhinoLocalEmulated(
      "rhino-local: openidm.query: You must use exactly one of [_queryId, _queryExpression, _queryFilter]."
    );
  }
  if (params._queryExpression !== undefined) {
    throw __rhinoLocalEmulated(
      "rhino-local: openidm.query: Query Expressions are not supported when using DS as the repo"
    );
  }
  // query-all-ids is the one _queryId measured: every record, _id/_rev only.
  if (params._queryId !== undefined && params._queryId !== "query-all-ids") {
    throw __rhinoLocalFault("openidm-query-id", 
      "rhino-local: not mocked: openidm.query _queryId " + String(params._queryId)
    );
  }
  if (fields === undefined && params._fields !== undefined) {
    fields = Array.isArray(params._fields) ? params._fields : [String(params._fields)];
  }
  var filter = params._queryId !== undefined ? "true" : params._queryFilter;
  if (params._queryId !== undefined) {
    fields = ["_id"];
  }
  // Parsed before the rows, as AIC parses it before reading any: a malformed
  // filter fails on an empty collection too.
  var lane = __rhinoLocalCrestFilter.scriptTesterLane(collection);
  var ast = __rhinoLocalParseFilter(filter, lane);
  // A leaf AIC judges before reading any record (a field rule, its 400s):
  // judged here even on an empty collection, so it cannot pass vacuously.
  var assertion = __rhinoLocalCrestFilter.assertions(ast, lane);
  if (assertion !== null && assertion.illegal !== undefined) {
    throw __rhinoLocalIllegalAssertion(assertion.illegal);
  }
  if (assertion !== null) {
    __rhinoLocalFilterUnsupported(filter, assertion.unknown.op + " on " + assertion.unknown.field + " " +
      __rhinoLocalUndecidedWhy[assertion.unknown.dimension]);
  }
  var result = [];
  var i;
  for (i = 0; i < rows.length; i += 1) {
    var verdict = __rhinoLocalCrestFilter.evaluate(ast, rows[i], lane);
    if (verdict !== true && verdict !== false) {
      __rhinoLocalFilterUnsupported(filter,
        "whether record " + JSON.stringify(String(rows[i]._id)) + " matches: " +
          verdict.unknown.op + " on " + verdict.unknown.field + " " +
          __rhinoLocalUndecidedWhy[verdict.unknown.dimension]);
    }
    if (verdict === true) {
      __rhinoLocalBagAccess("query", collection + "/" + rows[i]._id, rows[i], fields, params);
      result.push(__rhinoLocalProject(rows[i], fields, { method: "query", resource: collection }));
    }
  }
  // Key order as for-in and Object.keys see it on AIC, the same on every
  // response (openidm-query-response-order probe, 2026-11-09). AIC's
  // JSON.stringify of a response writes the keys in an order that varies
  // between calls, so this is the one stable order there is.
  return {
    pagedResultsCookie: null,
    totalPagedResultsPolicy: "NONE",
    totalPagedResults: -1,
    resultCount: result.length,
    result: __rhinoLocalOpenidmList(result),
  };
};

openidm.create = function (resourceName, newResourceId, content) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw __rhinoLocalArityRefusal("openidm.create", arguments, "3..5");
  }
  var collection = String(resourceName);
  var id = newResourceId;
  var recordedResource;
  if (id === null || id === undefined || id === "") {
    __rhinoLocal.generatedId += 1;
    id = "generated-" + __rhinoLocal.generatedId;
    recordedResource = collection;
  } else {
    id = String(id);
    recordedResource = collection + "/" + id;
  }
  __rhinoLocalPushOpenidm("create", recordedResource, content);
  __rhinoLocalInvalidDefault("create", collection);
  if (__rhinoLocalAbsentType(collection)) {
    throw __rhinoLocalUnknownTypeFault("create", collection);
  }
  // With a profile, an unseeded resource the profile does not speak for (not
  // `managed/<type>`) is a missing fixture, as read and query are. Without a
  // profile create still makes the collection (pre-existing; gaps doc).
  if (
    !__rhinoLocalHas(__rhinoLocal.managed, collection) &&
    __rhinoLocal.knownTypes &&
    !__rhinoLocalIsManagedType(collection)
  ) {
    __rhinoLocalRequireCollection("create", collection);
  }
  if (!__rhinoLocalHas(__rhinoLocal.managed, collection)) {
    __rhinoLocal.managed[collection] = [];
  }
  var rows = __rhinoLocal.managed[collection];
  // Before the duplicate check: on AIC the create chain (hooks, policy) runs
  // before the record is committed and found to collide.
  __rhinoLocalManagedWriteFault("create", collection);
  var existing = __rhinoLocalFindRecord(collection, id).record;
  // openidm.create sends an undefined value as null (docs/api/12), so judge
  // the serialised content. Policy runs before the collision is found.
  __rhinoLocalImplicitPolicy("create", collection, __rhinoLocalCloneUndefinedAsNull(content, true), !existing);
  if (existing) {
    __rhinoLocalBagAccess("create", collection + "/" + id, existing);
    throw __rhinoLocalEmulated(
      "rhino-local: openidm.create: Entry Already Exists: " + recordedResource
    );
  }
  var stored = __rhinoLocalCloneUndefinedAsNull(content, true);
  // Defaults land on a create that omits the property (docs/api/10).
  __rhinoLocalApplyDefaults(collection, stored);
  stored = __rhinoLocalOpenidmRecord(
    stored,
    id,
    stored._rev === undefined ? "local-rev" : stored._rev
  );
  __rhinoLocalBagIdmWrite("create", collection + "/" + stored._id, stored);
  rows.push(stored);
  return __rhinoLocalProject(stored, arguments.length >= 5 ? arguments[4] : null, { method: "create", resource: String(resourceName) });
};

openidm.update = function (id, _rev, value) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw __rhinoLocalArityRefusal("openidm.update", arguments, "3..5");
  }
  var resource = String(id);
  __rhinoLocalPushOpenidm("update", resource, value);
  __rhinoLocalInvalidDefault("update", __rhinoLocalSplitResource(resource).collection);
  var found = __rhinoLocalRequireRecord("update", resource);
  __rhinoLocalBagAccess("update", resource, found.record);
  __rhinoLocalCheckRev("update", _rev, found.record);
  __rhinoLocalManagedWriteFault("update", __rhinoLocalSplitResource(resource).collection);
  __rhinoLocalImplicitPolicy("update", __rhinoLocalSplitResource(resource).collection, value, false);
  var next = __rhinoLocalClone(value);
  next = __rhinoLocalOpenidmRecord(next, found.record._id, found.record._rev);
  __rhinoLocalBagIdmWrite("update", resource, next);
  found.rows[found.index] = next;
  return __rhinoLocalProject(next, arguments.length >= 5 ? arguments[4] : null, { method: "update", resource: String(id) });
};

openidm.patch = function (resourceName, _rev, patch) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw __rhinoLocalArityRefusal("openidm.patch", arguments, "3..5");
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("patch", resource, patch);
  __rhinoLocalInvalidDefault("patch", __rhinoLocalSplitResource(resource).collection);
  var i;
  var op;
  var field;
  for (i = 0; i < patch.length; i += 1) {
    op = patch[i];
    if (op.operation !== "add" && op.operation !== "remove" && op.operation !== "replace") {
      throw __rhinoLocalEmulated(
        "rhino-local: openidm.patch: not a valid JSON patch (operation " +
          String(op.operation) +
          ")"
      );
    }
  }
  // Still parse-time: `replace` with null is a measured 400 "not a valid JSON
  // patch" over REST, on a defaulted boolean and on an undefaulted
  // relationship (docs/api/10); the script path is assumed to answer in the
  // same family, as for an unknown operation. `add` with null is unmeasured.
  // A `value` that is undefined, or no `value` key at all, is the same null:
  // AIC sends a script's undefined as null (docs/api/12, measured for
  // openidm.create content and for httpClient bodies at every depth), and an
  // operation without a value is a null value to the patch parser.
  for (i = 0; i < patch.length; i += 1) {
    op = patch[i];
    if (op.operation === "remove" || (op.value !== null && op.value !== undefined)) {
      continue;
    }
    if (op.operation === "replace") {
      throw __rhinoLocalEmulated(
        "rhino-local: openidm.patch: not a valid JSON patch (replace " + String(op.field) + " with null)"
      );
    }
    if (op.operation === "add") {
      throw __rhinoLocalFault("openidm-patch-add-null",
        "rhino-local: openidm.patch: add " + String(op.field) + " with null: AIC refuses replace with null as an " +
          "invalid JSON patch (docs/api/10), and what it does with add is unmeasured");
    }
  }
  // The patch is parsed before the record is looked up or its revision
  // checked (CREST parses a request body before routing it; structural over
  // REST, assumed for the script path), so a malformed patch on an absent
  // record or a stale revision is still the patch error.
  var found = __rhinoLocalRequireRecord("patch", resource);
  __rhinoLocalBagAccess("patch", resource, found.record);
  __rhinoLocalCheckRev("patch", _rev, found.record);
  var collection = __rhinoLocalSplitResource(resource).collection;
  __rhinoLocalManagedWriteFault("patch", collection);
  __rhinoLocalRemovedDefault(collection, patch);
  // Policy judges the top-level values the patch writes; nulls were refused
  // above, and a deeper path writes inside a property, which is not checked.
  var written = {};
  for (i = 0; i < patch.length; i += 1) {
    field = String(patch[i].field).replace(/^\//, "");
    if (patch[i].operation !== "remove" && field.indexOf("/") < 0) {
      __rhinoLocalSetOwn(written, field, patch[i].value);
    }
  }
  __rhinoLocalImplicitPolicy("patch", collection, written, false);
  var next = __rhinoLocalClone(found.record);
  for (i = 0; i < patch.length; i += 1) {
    op = patch[i];
    field = String(op.field).replace(/^\//, "");
    if (op.operation === "remove") {
      delete next[field];
    } else {
      __rhinoLocalSetOwn(next, field, op.value);
    }
  }
  __rhinoLocalBagIdmWrite("patch", resource, next);
  Object.keys(found.record).forEach(function (key) { delete found.record[key]; });
  Object.keys(next).forEach(function (key) { __rhinoLocalSetOwn(found.record, key, next[key]); });
  return __rhinoLocalProject(found.record, arguments.length >= 5 ? arguments[4] : null, { method: "patch", resource: String(resourceName) });
};

openidm.delete = function (resourceName, _rev) {
  if (arguments.length < 2 || arguments.length > 4) {
    throw __rhinoLocalArityRefusal("openidm.delete", arguments, "2..4");
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("delete", resource);
  __rhinoLocalInvalidDefault("delete", __rhinoLocalSplitResource(resource).collection);
  var found = __rhinoLocalRequireRecord("delete", resource);
  __rhinoLocalBagAccess("delete", resource, found.record);
  if (_rev !== null && _rev !== undefined && String(_rev) !== String(found.record._rev)) {
    throw __rhinoLocalEmulated(
      "rhino-local: openidm.delete: Assertion Failed: the entry cannot be removed because the request contained an LDAP assertion control and the associated filter did not match the contents of the entry"
    );
  }
  __rhinoLocalManagedWriteFault("delete", __rhinoLocalSplitResource(resource).collection);
  var removed = found.record;
  __rhinoLocalBagIdmWrite("delete", resource, removed);
  found.rows.splice(found.index, 1);
  return __rhinoLocalProject(removed, arguments.length >= 4 ? arguments[3] : null, { method: "delete", resource: String(resourceName) });
};

openidm.action = function (resource, actionName, content, params) {
  if (arguments.length < 2 || arguments.length > 5) {
    throw __rhinoLocalArityRefusal("openidm.action", arguments, "2..5");
  }
  var body = content;
  if (body === undefined || body === null) {
    body = params;
  }
  __rhinoLocalPushOpenidm("action", String(resource), body, String(actionName));
  var managedCollection = /^managed\/[^/]+$/.test(String(resource));
  // Checked before the measured action-name refusal: which of the two AIC
  // raises for an absent type is unmeasured, and a stub cannot make it exist.
  var managedType = /^(managed\/[^/]+)(?:\/|$)/.exec(String(resource));
  if (managedType) {
    __rhinoLocalInvalidDefault("action", managedType[1]);
  }
  if (managedType && __rhinoLocalAbsentType(managedType[1])) {
    throw __rhinoLocalUnknownTypeFault("action", managedType[1]);
  }
  // Measured on a managed collection only; other resources have other actions.
  if (
    managedCollection &&
    actionName !== "patch" &&
    actionName !== "triggerSyncCheck" &&
    actionName !== "updateLastSync"
  ) {
    throw __rhinoLocalEmulated(
      "rhino-local: openidm.action: Expecting String containing one of: patch triggerSyncCheck updateLastSync"
    );
  }
  var i;
  for (i = 0; i < __rhinoLocal.openidmActions.length; i += 1) {
    var stub = __rhinoLocal.openidmActions[i];
    if (stub.match.action === String(actionName) && __rhinoLocalMatchPattern(stub.match.resource, String(resource), "openidmActions", "resource")) {
      return __rhinoLocalClone(stub.reply.body);
    }
  }
  // patch, triggerSyncCheck and updateLastSync on a managed collection used to
  // return {} here. On AIC a collection patch runs the per-record write chain
  // and the sync actions run the type's mappings; this lane does neither, and
  // what each returns is unmeasured (E15b).
  if (managedCollection) {
    throw __rhinoLocalFault("openidm-managed-collection-action/" + String(actionName),
      "rhino-local: openidm.action: " + String(actionName) + " on " + JSON.stringify(String(resource)) +
        " is not emulated: on AIC it runs the records' write chain or the type's sync mappings, and its result is unmeasured. " +
        "Stub it with given.openidmActions, or expect the fault (E19)");
  }
  throw __rhinoLocalFixtureFault("openidm-action", 
    "rhino-local: openidm.action: no given.openidmActions stub for " +
      String(actionName) + " " + String(resource)
  );
};

function __rhinoLocalAsPattern(value) {
  if (value && typeof value === "object" && typeof value.__regex === "string") {
    return new RegExp(value.__regex, value.__flags || "");
  }
  return value;
}

function __rhinoLocalMatchPattern(pattern, value, channel, field) {
  var p = __rhinoLocalAsPattern(pattern);
  if (typeof p === "string") {
    return p === value;
  }
  if (p && typeof p.test === "function") {
    return p.test(value);
  }
  throw __rhinoLocalFixtureFault("match-pattern-type", "rhino-local: given." + channel + " match." + field + " is not a string or regexp");
}

function __rhinoLocalMatchHttp(url, method) {
  var i;
  var stub;
  var wantMethod;
  for (i = 0; i < __rhinoLocal.httpStubs.length; i += 1) {
    stub = __rhinoLocal.httpStubs[i];
    if (!__rhinoLocalMatchPattern(stub.match.url, url, "http", "url")) {
      continue;
    }
    wantMethod = stub.match.method;
    if (wantMethod && String(wantMethod).toUpperCase() !== method) {
      continue;
    }
    return __rhinoLocalClone(stub.reply);
  }
  return null;
}

httpClient.send = function (uri, requestOptions) {
  if (arguments.length < 1 || arguments.length > 2) {
    throw __rhinoLocalArityRefusal("httpClient.send", arguments, "1 or 2");
  }
  var url = String(uri);
  var method = "GET";
  if (requestOptions && requestOptions.method) {
    method = String(requestOptions.method).toUpperCase();
  }
  var effect = { url: url, method: method };
  // A `body` key holding undefined is sent as null, not omitted (docs/api/12
  // §3, measured); see __rhinoLocalSendBody for the rest of the conversion.
  if (requestOptions && typeof requestOptions === "object" && "body" in requestOptions) {
    effect.body = __rhinoLocalSendBody(requestOptions.body, "body", []);
    if (effect.body === undefined) {
      delete effect.body;
    }
  }
  __rhinoLocal.http.push(effect);
  var reply = __rhinoLocalMatchHttp(url, method);
  if (!reply) {
    throw __rhinoLocalFixtureFault("http", 
      "rhino-local: httpClient.send: no given.http stub for " + method + " " + url
    );
  }
  var response = {
    status: reply.status,
    ok: reply.status >= 200 && reply.status < 300,
    statusText: String(reply.status),
    headers: reply.headers || {},
    json: function () {
      return reply.body;
    },
    text: function () {
      if (typeof reply.body === "string") {
        return reply.body;
      }
      if (reply.body === undefined) {
        return "";
      }
      return JSON.stringify(reply.body);
    },
  };
  return {
    get: function () {
      return response;
    },
  };
};

// How store() lays an AM attribute out in IDM. Measured 2026-10-01 on alpha
// (live-identity-store-visibility, live-identity-store-families): one
// representative per fr-attr family, plus the standard attributes below. N in
// a family is assumed from its measured representative (istr1 for istr1..20).
// `none` / `one` / `many` say what 0, 1 and 2+ values become:
//   "remove" (the property reads null), "empty" ([]), "scalar", "array",
//   "violation" (store throws ldap errorcode=65, nothing applied) or
//   "unmeasured" (the local lane refuses).
// `syntax` values that DS rejects throw ldap errorcode=21, nothing applied.
// Inventory and collision policy are shared with Node validation via preamble.
var __rhinoLocalIdentityFamilies = __rhinoLocalIdentityPolicy.families;
var __rhinoLocalIdentityStandard = __rhinoLocalIdentityPolicy.standard;
var __rhinoLocalIdentityMeasuredNames =
  "fr-attr-str1..5, fr-attr-istr1..20, fr-attr-multi1..5, fr-attr-imulti1..5, fr-attr-int1..5, " +
  "fr-attr-iint1..5, fr-attr-date1..5, fr-attr-idate1..5, givenName, sn, mail, telephoneNumber, fr-idm-custom-attrs";

function __rhinoLocalCustomBag(resource, record) {
  if (__rhinoLocalHas(__rhinoLocal.identityCustomAttrs, resource)) {
    return __rhinoLocal.identityCustomAttrs[resource].slice();
  }
  var bag = {};
  Object.keys(record).forEach(function (key) {
    if (key.indexOf("custom_") === 0) {
      bag[key] = record[key];
    }
  });
  return [JSON.stringify(bag)];
}

// Clearing a value must not forget that the bag is this key's storage location.
function __rhinoLocalBagOwnedKeys(resource, record) {
  var keys = (__rhinoLocal.identityCustomAttrsOwnedKeys[resource] || []).slice();
  var bag = __rhinoLocalBagObject(__rhinoLocalCustomBag(resource, record));
  if (__rhinoLocalIsPlainObject(bag)) {
    Object.keys(bag).forEach(function (key) {
      if (keys.indexOf(key) === -1) { keys.push(key); }
    });
  }
  return keys.sort();
}

function __rhinoLocalBagObject(values) {
  return values.length === 0 ? {} : JSON.parse(values[0]);
}

function __rhinoLocalBagCategory(bag) {
  if (bag === null) { return "null"; }
  if (Array.isArray(bag)) { return "array"; }
  return typeof bag;
}

function __rhinoLocalBagAccess(method, resource, record, fields, params) {
  var bag = __rhinoLocalBagObject(__rhinoLocalCustomBag(resource, record));
  var category = __rhinoLocalBagCategory(bag);
  if (category === "object") {
    return;
  }
  var fullRead = method === "read" && (fields === undefined || fields === null);
  if (fullRead && category === "null") {
    return;
  }
  // Full reads: string/number/array measured in edges; boolean in nonobject.
  // Delete: only a string bag was measured. Other categories stay refused.
  if ((fullRead && ["string", "number", "array", "boolean"].indexOf(category) !== -1) ||
      (method === "delete" && category === "string")) {
    // Measured text; no status was captured for it, so no e.javaException
    // (review pf-02 P3-3).
    throw __rhinoLocalJavaException("org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Response is not application/json");
  }
  if (method === "query" && ["string", "number", "array", "null"].indexOf(category) !== -1 &&
      params._queryFilter !== undefined && Array.isArray(fields) && fields.length === 1 && fields[0] === "_id") {
    return;
  }
  throw __rhinoLocalFault("identity-bag-non-object-access", 
    "rhino-local: openidm." + method + ": " + resource +
      " carries a non-object fr-idm-custom-attrs bag (" + category + "); this IDM access is unmeasured; restore an object bag through AM before this access"
  );
}

// One lifecycle path for persisted bag provenance and resource incarnations.
function __rhinoLocalBagIdmWrite(method, resource, record) {
  if (method === "create" || method === "delete") {
    delete __rhinoLocal.identityCustomAttrs[resource];
    delete __rhinoLocal.identityCustomAttrsOwnedKeys[resource];
    __rhinoLocal.identityGenerations[resource] = (__rhinoLocal.identityGenerations[resource] || 0) + 1;
    return;
  }
  var keys = __rhinoLocalBagOwnedKeys(resource, record);
  keys.forEach(function (key) { __rhinoLocalCheckBagKey(key, "openidm." + method); });
  var bag = {};
  Object.keys(record).forEach(function (key) {
    if (key.indexOf("custom_") === 0 || keys.indexOf(key) !== -1) {
      bag[key] = record[key];
      if (keys.indexOf(key) === -1) { keys.push(key); }
    }
  });
  // Even an ordinary IDM write restores an absent bag to one {} (measured).
  __rhinoLocal.identityCustomAttrs[resource] = [JSON.stringify(bag)];
  __rhinoLocal.identityCustomAttrsOwnedKeys[resource] = keys.sort();
}

function __rhinoLocalIdentityShape(field, cardinality, syntax) {
  return cardinality === "multi"
    ? { field: field, none: "empty", one: "array", many: "array", syntax: syntax }
    : { field: field, none: "remove", one: "scalar", many: "violation", syntax: syntax };
}

// A declaration (given.identityAttributes) wins over a measurement: it is the
// suite's statement of this tenant's mapping, and only the local lane reads it.
function __rhinoLocalIdentityLayout(name) {
  var key = String(name);
  var declared = __rhinoLocal.identityAttributes || {};
  if (__rhinoLocalHas(declared, key)) {
    var shape = __rhinoLocalIdentityShape(declared[key].field, declared[key].cardinality, null);
    // What 2+ values do to an attribute nobody measured is DS schema detail
    // the declaration does not carry.
    if (shape.many === "violation") {
      shape.many = "unmeasured";
    }
    shape.declared = true;
    return shape;
  }
  if (key === "fr-idm-custom-attrs") {
    return { field: key, none: "bag", one: "bag", many: "violation", syntax: "bag" };
  }
  var i;
  var match;
  for (i = 0; i < __rhinoLocalIdentityFamilies.length; i += 1) {
    match = __rhinoLocalIdentityFamilies[i][0].exec(key);
    if (match) {
      return __rhinoLocalIdentityShape(
        __rhinoLocalIdentityFamilies[i][1] + match[1],
        __rhinoLocalIdentityFamilies[i][2],
        __rhinoLocalIdentityFamilies[i][3]
      );
    }
  }
  if (__rhinoLocalHas(__rhinoLocalIdentityStandard, key)) {
    var standard = __rhinoLocalIdentityStandard[key];
    return {
      field: standard.field, none: standard.none, one: standard.one,
      many: standard.many, syntax: standard.syntax
    };
  }
  return null;
}

// Collision behavior is unmeasured; declarations may override eligible bag keys.
// Two ids: a bag key naming another attribute, and a whole-bag replacement
// sharing one store with a write the replacement also covers.
function __rhinoLocalBagCollision(method, property, reason, id) {
  var binding = method.indexOf("openidm.") === 0 ? method : "idRepository.getIdentity()." + method;
  throw __rhinoLocalFault(id || "identity-bag-key-collision", "rhino-local: " + binding + ": fr-idm-custom-attrs key " +
    JSON.stringify(property) + " collides with " + reason + "; this identity mapping is unmeasured; use non-overlapping bag keys and identityAttributes fields");
}
function __rhinoLocalCheckBagKey(property, method) {
  var reason = __rhinoLocalIdentityPolicy.collisionReason(property);
  if (reason !== null) {
    __rhinoLocalBagCollision(method || "store", property, reason);
  }
}
function __rhinoLocalCheckBagKeys(bag) {
  if (!__rhinoLocalIsPlainObject(bag)) { return; }
  Object.keys(bag).forEach(function (property) { __rhinoLocalCheckBagKey(property); });
}

// Preflight the whole store on copies. A declared key write changes the object
// bag alongside the record, including when [] previously made the bag absent.
// Nothing is applied or recorded until every transition is representable.
function __rhinoLocalPlanIdentityStore(resource, record, plan) {
  var next = __rhinoLocalClone(record);
  var values = __rhinoLocalCustomBag(resource, record);
  var previous = __rhinoLocalBagObject(values);
  var keys = __rhinoLocalBagOwnedKeys(resource, record);
  var changed = false;
  var replacement = plan.filter(function (step) { return step.rule === "bag"; })[0];
  if (replacement) {
    plan.forEach(function (step) {
      if (step === replacement) { return; }
      var property = step.layout.field;
      var incoming = replacement.converted[0];
      if (property.indexOf("custom_") === 0 ||
          keys.indexOf(property) !== -1 ||
          (__rhinoLocalIsPlainObject(incoming) && __rhinoLocalHas(incoming, property))) {
        __rhinoLocalBagCollision("store", property, "a bag replacement and another attribute write in the same store",
          "identity-bag-replacement-overlap");
      }
    });
  }
  plan.forEach(function (step) {
    if (step.rule === "bag") {
      Object.keys(next).forEach(function (property) {
        if (property.indexOf("custom_") === 0 || keys.indexOf(property) !== -1) {
          delete next[property];
        }
      });
      var bag = step.converted[0];
      if (__rhinoLocalIsPlainObject(bag)) {
        Object.keys(bag).forEach(function (property) {
          next[property] = bag[property];
          if (keys.indexOf(property) === -1) { keys.push(property); }
        });
      }
      values = step.given.slice();
      previous = __rhinoLocalBagObject(values);
      changed = true;
      return;
    }
    var property = step.layout.field;
    var bagKey = property.indexOf("custom_") === 0 || keys.indexOf(property) !== -1;
    if (bagKey && !__rhinoLocalIsPlainObject(previous)) {
      throw __rhinoLocalFault("identity-store-non-object-bag", "rhino-local: idRepository.getIdentity().store: replacing " +
        JSON.stringify(property) + " within a non-object fr-idm-custom-attrs bag (" +
        __rhinoLocalBagCategory(previous) + ") is unmeasured; restore an object bag through AM in a separate store before this write");
    }
    if (step.rule === "remove") {
      delete next[property];
    } else if (step.rule === "empty") {
      next[property] = [];
    } else if (step.rule === "scalar") {
      next[property] = step.converted[0];
    } else {
      next[property] = step.converted.slice();
    }
    if (bagKey) {
      if (keys.indexOf(property) === -1) { keys.push(property); }
      __rhinoLocalCheckBagKeys(previous);
      __rhinoLocalCheckBagKey(property);
      if (__rhinoLocalHas(next, property)) {
        previous[property] = next[property];
      } else {
        delete previous[property];
      }
      values = [JSON.stringify(previous)];
      changed = true;
    }
  });
  if (next._id !== record._id) {
    throw __rhinoLocalFault("identity-store-id-layout-change", "rhino-local: idRepository.getIdentity().store: changing the identity resource through an _id layout is unmeasured; keep the existing _id");
  }
  return { record: next, bag: values, ownedKeys: keys.sort(), changed: changed };
}

// The exception AM's store() throws when DS refuses the modify (measured text).
function __rhinoLocalIdentityUpdateError(code) {
  return __rhinoLocalJavaException(
    "org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper$IdentityUpdateException: " +
      "Exception persisting attribute: Plug-in org.forgerock.openam.idrepo.ldap.DJLDAPv3Repo " +
      "encountered a ldap exception.  ldap errorcode=" + code
  );
}

// Script value -> IDM value for a typed family. Returns {value} when stored,
// {code: 21} when DS refuses it, {unmeasured: reason} otherwise.
function __rhinoLocalIdentityConvert(syntax, raw) {
  var text = String(raw);
  var match;
  if (syntax === "bag") {
    var bag;
    try {
      bag = JSON.parse(text);
    } catch (_error) {
      return { code: 21 };
    }
    return { value: bag };
  }
  if (syntax === "integer") {
    // Measured: "42" -> 42. "abc" and "4.5" -> errorcode=21.
    if (/^(0|[1-9][0-9]*)$/.test(text) && Number(text) <= 9007199254740991) {
      return { value: Number(text) };
    }
    if (/^[+-]?[0-9]+$/.test(text)) {
      return { unmeasured: "a signed, zero-padded or out-of-range integer" };
    }
    return { code: 21 };
  }
  if (syntax === "date") {
    // Measured: "20261001120000Z" -> "2026-10-01T12:00:00Z". An ISO
    // timestamp, a bare date and free text -> errorcode=21.
    match = /^([0-9]{4})([0-9]{2})([0-9]{2})([0-9]{2})([0-9]{2})([0-9]{2})Z$/.exec(text);
    if (match) {
      var iso = match[1] + "-" + match[2] + "-" + match[3] + "T" + match[4] + ":" + match[5] + ":" + match[6] + "Z";
      var parsed = new Date(iso);
      if (!isNaN(parsed.getTime()) && parsed.toISOString().slice(0, 19) + "Z" === iso) {
        return { value: iso };
      }
      return { unmeasured: "an out-of-range GeneralizedTime" };
    }
    if (/^[0-9]{10}([0-9]{2}([0-9]{2})?)?([.,][0-9]+)?(Z|[+-][0-9]{2}([0-9]{2})?)$/.test(text)) {
      return { unmeasured: "a GeneralizedTime other than YYYYMMDDHHMMSSZ" };
    }
    return { code: 21 };
  }
  return { value: text };
}

// IDM value -> what getAttributeValues returns: always strings, and a date in
// the GeneralizedTime the script wrote (measured round trip).
function __rhinoLocalIdentityRender(syntax, value, name) {
  if (syntax === "date") {
    var match = /^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})Z$/.exec(String(value));
    if (!match) {
      throw __rhinoLocalFault("identity-date-rendering", 
        "rhino-local: idRepository.getIdentity().getAttributeValues: how AM renders the IDM date " +
          JSON.stringify(String(value)) + " of " + JSON.stringify(String(name)) +
          " is unmeasured; seed it as YYYY-MM-DDTHH:MM:SSZ"
      );
    }
    return match[1] + match[2] + match[3] + match[4] + match[5] + match[6] + "Z";
  }
  return String(value);
}

idRepository.getIdentity = function (userName) {
  __rhinoLocalExpectArity("idRepository.getIdentity", arguments, 1);
  var wanted = String(userName);
  var collections = Object.keys(__rhinoLocal.managed);
  var c;
  var rows;
  var i;
  var record;
  var found = null;
  var resource = null;
  for (c = 0; c < collections.length; c += 1) {
    rows = __rhinoLocal.managed[collections[c]];
    for (i = 0; i < rows.length; i += 1) {
      record = rows[i];
      if (String(record._id) === wanted) {
        found = record;
        resource = collections[c] + "/" + wanted;
        break;
      }
    }
    if (found) {
      break;
    }
  }
  var generation = resource === null ? 0 : (__rhinoLocal.identityGenerations[resource] || 0);
  function refresh(method) {
    if (resource !== null) {
      var split = __rhinoLocalSplitResource(resource);
      found = __rhinoLocalFindRecord(split.collection, split.recordId).record;
      if (found && generation !== (__rhinoLocal.identityGenerations[resource] || 0)) {
        throw __rhinoLocalFault("identity-wrapper-after-recreate", "rhino-local: idRepository.getIdentity()." + method + ": retained wrapper behavior after delete and recreate is unmeasured; get a fresh identity wrapper");
      }
    }
  }
  function missing(method) {
    var error = new Error(
      'Cannot invoke "org.forgerock.openam.scripting.api.ScriptedIdentity.' +
        method + '" because "this.amIdentity" is null'
    );
    error.name = "InternalError";
    throw error;
  }
  // Keyed by the AM attribute name the script used.
  var pending = {};
  var IDM_NAME = "<not-an-AM-attribute>";
  function field(name) {
    var key = String(name);
    if (key.indexOf("custom_") === 0) {
      return IDM_NAME;
    }
    var layout = __rhinoLocalIdentityLayout(key);
    if (layout) {
      return layout.field;
    }
    if (key === "uid") {
      return "userName";
    }
    if (key === "inetUserStatus") {
      return "accountStatus";
    }
    if (key === "fr-idm-uuid") {
      return "_id";
    }
    if (
      key === "userName" || key === "accountStatus" ||
      /^fr(Unindexed|Indexed)(String|Multivalued|Integer|Date)[0-9]+$/.test(key)
    ) {
      return IDM_NAME;
    }
    return key;
  }
  function amName(idmName) {
    var key = String(idmName);
    var families = {
      UnindexedString: "str", IndexedString: "istr", UnindexedMultivalued: "multi",
      IndexedMultivalued: "imulti", UnindexedInteger: "int", IndexedInteger: "iint",
      UnindexedDate: "date", IndexedDate: "idate"
    };
    var match = /^fr(Unindexed|Indexed)(String|Multivalued|Integer|Date)([0-9]+)$/.exec(key);
    if (match) {
      return "fr-attr-" + families[match[1] + match[2]] + match[3];
    }
    if (key.indexOf("custom_") === 0) {
      return "fr-idm-custom-attrs";
    }
    return key === "userName" ? "uid" : key === "accountStatus" ? "inetUserStatus" : key;
  }
  function unmeasured(method, name, detail) {
    return __rhinoLocalFault("identity-attribute-layout", 
      "rhino-local: idRepository.getIdentity()." + method + ": " + detail +
        "; measured: " + __rhinoLocalIdentityMeasuredNames +
        ". Declare a tenant attribute's IDM layout with identityAttributes: { " +
        JSON.stringify(String(name)) +
        ': { field: "<IDM property>", cardinality: "single" | "multi" } } ' +
        "(defineSuite always, .run().identityAttributes(), or given.identityAttributes)"
    );
  }
  // A wrong-name READ is measured (no values); what store() does with a
  // wrong-name WRITE is not, so the local lane refuses one rather than guess.
  function writable(method, name) {
    var key = field(name);
    if (key === IDM_NAME) {
      throw __rhinoLocalFault("identity-write-idm-field-name", 
        "rhino-local: idRepository.getIdentity()." + method + ": " +
          JSON.stringify(String(name)) + " is the IDM field name; AM names this attribute " +
          JSON.stringify(amName(name)) + ". What AM stores for an unknown attribute name is unmeasured, so the local lane refuses it"
      );
    }
    if (String(name) === "cn" && !__rhinoLocalHas(__rhinoLocal.identityAttributes || {}, "cn")) {
      // Measured: AM stores cn and reads it back, but IDM's managed user has
      // no cn property, so the value lives outside the record this lane keeps.
      throw __rhinoLocalFault("identity-write-cn", 
        "rhino-local: idRepository.getIdentity()." + method +
          ': AM stores "cn" outside the IDM managed record (measured), which the local lane does not model, so it refuses the write'
      );
    }
    var layout = __rhinoLocalIdentityLayout(name);
    if (layout === null) {
      throw unmeasured(method, name, "how AM stores " + JSON.stringify(String(name)) + " in IDM is unmeasured, so the local lane refuses the write");
    }
    return layout;
  }
  function values(name) {
    var key = field(name);
    var layout = __rhinoLocalIdentityLayout(name);
    if (key === IDM_NAME) {
      return [];
    }
    // AM reads the persisted identity until store(), even after setAttribute.
    if (layout && layout.syntax === "bag") {
      return __rhinoLocalCustomBag(resource, found);
    }
    var value = found[key];
    if (value === undefined && key === "accountStatus") {
      value = found.inetUserStatus;
    }
    if (value === undefined || value === null) {
      return [];
    }
    var syntax = layout ? layout.syntax : null;
    var list = Array.isArray(value) ? value : [value];
    return list.map(function (entry) {
      return __rhinoLocalIdentityRender(syntax, entry, name);
    });
  }
  return {
    getName: function () {
      refresh("getName");
      if (!found) {
        return wanted;
      }
      if (found.userName !== undefined) {
        return found.userName;
      }
      return found._id;
    },
    getUniversalId: function () {
      refresh("getUniversalId");
      return found ? found._id : wanted;
    },
    exists: function () {
      refresh("exists");
      return Boolean(found);
    },
    getAttributeValues: function (attributeName) {
      refresh("getAttributeValues");
      if (!found) {
        missing("getAttributeValues(String)");
      }
      if (__rhinoLocalJavaListHost !== null) {
        return __rhinoLocalListView(values(attributeName));
      }
      var list = __rhinoLocalJavaList(values(attributeName));
      __rhinoLocalHide(list, "includes", undefined);
      __rhinoLocalHide(list, "toString", function () {
        return "[" + this.join(", ") + "]";
      });
      return list;
    },
    setAttribute: function (attributeName, attributeValues) {
      refresh("setAttribute");
      if (!found) {
        missing("setAttribute(String, String[])");
      }
      __rhinoLocalExpectArity("idRepository.getIdentity().setAttribute", arguments, 2);
      if (!Array.isArray(attributeValues)) {
        throw __rhinoLocalFault("identity-set-attribute-non-array", "rhino-local: setAttribute requires an array of strings");
      }
      writable("setAttribute", attributeName);
      pending[String(attributeName)] = attributeValues.map(String);
    },
    addAttribute: function (attributeName, attributeValue) {
      refresh("addAttribute");
      if (!found) {
        missing("addAttribute(String, String)");
      }
      __rhinoLocalExpectArity("idRepository.getIdentity().addAttribute", arguments, 2);
      writable("addAttribute", attributeName);
      var name = String(attributeName);
      var existing = __rhinoLocalHas(pending, name)
        ? pending[name].slice()
        : values(attributeName);
      var value = String(attributeValue);
      if (existing.indexOf(value) === -1) {
        existing.push(value);
      }
      pending[name] = existing;
    },
    store: function () {
      refresh("store");
      if (!found) {
        missing("storeAndThrow()");
      }
      __rhinoLocalExpectArity("idRepository.getIdentity().store", arguments, 0);
      // Recorded as an identity write, not an `openidm` patch: AM persists
      // through its identity repository, never the openidm binding, so IDM
      // failure stubs and openidm expectations must not see it. Updating the
      // local record is only so later reads in this run see the value.
      var names = Object.keys(pending);
      var plan = [];
      var codes = [];
      var i;
      var j;
      // Everything is checked before anything is applied: a store DS refuses
      // leaves every attribute in it unchanged (measured, one good and one
      // bad attribute in a single store()).
      for (i = 0; i < names.length; i += 1) {
        var layout = __rhinoLocalIdentityLayout(names[i]);
        var given = pending[names[i]];
        var rule = given.length === 0 ? layout.none : given.length === 1 ? layout.one : layout.many;
        if (rule === "unmeasured") {
          throw unmeasured(
            "store",
            names[i],
            "what AM stores for " + given.length + " values of " + JSON.stringify(names[i]) +
              " is unmeasured, so the local lane refuses it"
          );
        }
        var attributeCodes = [];
        if (rule === "violation" && layout.syntax !== "bag") {
          attributeCodes.push(65);
        }
        var converted = [];
        for (j = 0; j < given.length; j += 1) {
          var result = __rhinoLocalIdentityConvert(layout.syntax, given[j]);
          if (result.unmeasured) {
            throw unmeasured(
              "store",
              names[i],
              "how DS treats " + JSON.stringify(given[j]) + " (" + result.unmeasured + ") in " +
                JSON.stringify(names[i]) + " is unmeasured, so the local lane refuses it"
            );
          }
          if (result.code !== undefined) {
            if (attributeCodes.indexOf(result.code) === -1) {
              attributeCodes.push(result.code);
            }
          } else {
            converted.push(result.value);
          }
        }
        if (layout.syntax === "bag") {
          __rhinoLocalCheckBagKeys(__rhinoLocalBagObject(__rhinoLocalCustomBag(resource, found)));
          converted.forEach(__rhinoLocalCheckBagKeys);
        }
        // Malformed bag text wins over cardinality in either position (measured).
        if (layout.syntax === "bag" && rule === "violation" && attributeCodes.length === 0) {
          attributeCodes.push(65);
        }
        attributeCodes.forEach(function (code) {
          if (codes.indexOf(code) === -1) { codes.push(code); }
        });
        plan.push({ name: names[i], layout: layout, rule: rule, given: given, converted: converted });
      }
      if (codes.length > 1) {
        throw __rhinoLocalFault("identity-store-two-ds-rules", 
          "rhino-local: idRepository.getIdentity().store: this store breaks two DS rules at once (ldap errorcodes " +
            codes.join(" and ") + "); which one AM reports is unmeasured, so the local lane refuses it"
        );
      }
      if (codes.length === 1) {
        throw __rhinoLocalIdentityUpdateError(codes[0]);
      }
      var prepared = __rhinoLocalPlanIdentityStore(resource, found, plan);
      Object.keys(found).forEach(function (property) { delete found[property]; });
      Object.keys(prepared.record).forEach(function (property) { found[property] = prepared.record[property]; });
      if (prepared.changed) {
        __rhinoLocal.identityCustomAttrs[resource] = prepared.bag;
        __rhinoLocal.identityCustomAttrsOwnedKeys[resource] = prepared.ownedKeys;
      }
      for (i = 0; i < plan.length; i += 1) {
        var step = plan[i];
        __rhinoLocal.identityWrites.push({
          identity: String(found._id),
          attribute: step.name,
          values: step.given.slice()
        });
        delete pending[step.name];
      }
    },
    toString: function () {
      return "org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper@1b6d3586";
    },
  };
};

idRepository.createUser = function (userName, password, attributes) {
  if (arguments.length === 3 && __rhinoLocalIsPlainObject(attributes)) {
    var names = Object.keys(attributes);
    var i;
    for (i = 0; i < names.length; i += 1) {
      if (!Array.isArray(attributes[names[i]])) {
        throw new InternalError(
          "class java.lang.String cannot be cast to class java.util.Collection (java.lang.String and java.util.Collection are in module java.base of loader 'bootstrap')"
        );
      }
    }
  }
  throw new InternalError(
    "User creation through identity repository is not allowed in this environment"
  );
};

systemEnv = {
  getProperty: function (key, defaultValue, returnType) {
    if (arguments.length < 1 || arguments.length > 3) {
      throw __rhinoLocalArityRefusal("systemEnv.getProperty", arguments, "1..3");
    }
    var k = String(key);
    var declared = __rhinoLocal.esvProvided && __rhinoLocalHas(__rhinoLocal.esv, k);
    if (!declared && __rhinoLocal.esvUndeclared !== "absent") {
      throw __rhinoLocalFixtureFault("esv", 
        "rhino-local: systemEnv.getProperty: no given.esv entry for " +
          JSON.stringify(k) + "; declare it in given.esv or use given.esvUndeclared: \"absent\" (.esvUndeclared(\"absent\"))"
      );
    }
    var value = declared ? __rhinoLocal.esv[k] : null;
    if (value === null && arguments.length > 1) {
      value = defaultValue;
    }
    if (arguments.length < 3 || value === null) {
      return value;
    }
    var type;
    if (typeof returnType === "string") {
      type = returnType.toLowerCase();
    } else {
      var classType = String(returnType).match(/^\[JavaClass java\.lang\.(Integer|String|Boolean|Double)\]$/);
      if (!classType) {
        throw new InternalError("Property resolution failed");
      }
      type = classType[1].toLowerCase();
    }
    if (type === "string") {
      return String(value);
    }
    if (type === "integer" || type === "number") {
      var number = Number(value);
      if (isNaN(number)) {
        throw new InternalError("Property resolution failed");
      }
      return number;
    }
    if (type === "double") {
      var doubleValue = Number(value);
      if (isNaN(doubleValue)) {
        throw new InternalError("Property resolution failed");
      }
      return doubleValue;
    }
    if (type === "boolean") {
      return String(value).toLowerCase() === "true";
    }
    if (type === "object" || type === "map") {
      try {
        var parsed = JSON.parse(String(value));
        if (!__rhinoLocalIsPlainObject(parsed)) {
          throw new Error("not a map");
        }
        __rhinoLocalHide(parsed, "toString", function () {
          return __rhinoLocalJavaMapString(parsed).replace(/([0-9]+)\.0\b/g, "$1");
        });
        __rhinoLocalHide(parsed, "size", function () {
          return Object.keys(parsed).filter(function (name) {
            return name !== "toString" && name !== "size";
          }).length;
        });
        return parsed;
      } catch (e) {
        throw new InternalError("Property resolution failed");
      }
    }
    if (type === "array" || type === "list") {
      var values;
      try {
        values = String(value).split(",");
      } catch (e) {
        throw new InternalError("Property resolution failed");
      }
      var list = new java.util.ArrayList();
      for (var j = 0; j < values.length; j += 1) {
        list.add(values[j]);
      }
      return list;
    }
    throw new InternalError("Unsupported return type: " + returnType);
  },
};

function __rhinoLocalSecret(value) {
  return {
    getAsUtf8: function () {
      __rhinoLocalExpectArity("secret.getAsUtf8", arguments, 0);
      return value;
    },
  };
}

function __rhinoLocalRequireSecret(method, secretId) {
  var id = String(secretId);
  if (!__rhinoLocalHas(__rhinoLocal.secrets, id)) {
    if (__rhinoLocal.secretsProvided) {
      throw __rhinoLocalEmulated(
        "rhino-local: secrets." + method + ": Secret id " + id + " not accessible"
      );
    }
    throw __rhinoLocalFixtureFault("secrets", 
      "rhino-local: secrets." +
        method +
        ": no given.secrets entry for " +
        JSON.stringify(id)
    );
  }
  return __rhinoLocalSecret(__rhinoLocal.secrets[id]);
}

secrets.getGenericSecret = function (secretId) {
  __rhinoLocalExpectArity("secrets.getGenericSecret", arguments, 1);
  return __rhinoLocalRequireSecret("getGenericSecret", secretId);
};

secrets.getDecryptionKey = function (secretId) {
  __rhinoLocalExpectArity("secrets.getDecryptionKey", arguments, 1);
  return __rhinoLocalRequireSecret("getDecryptionKey", secretId);
};

secrets.getEncryptionKey = function (secretId) {
  __rhinoLocalExpectArity("secrets.getEncryptionKey", arguments, 1);
  return __rhinoLocalRequireSecret("getEncryptionKey", secretId);
};

secrets.getSigningKey = function (secretId) {
  __rhinoLocalExpectArity("secrets.getSigningKey", arguments, 1);
  return __rhinoLocalRequireSecret("getSigningKey", secretId);
};

secrets.getVerificationKey = function (secretId) {
  __rhinoLocalExpectArity("secrets.getVerificationKey", arguments, 1);
  return __rhinoLocalRequireSecret("getVerificationKey", secretId);
};

// given.bindings.cacheManager: {} is the measured state, no caches at all.
function __rhinoLocalRequireCaches(method) {
  if (!__rhinoLocalHas(__rhinoLocal.bindings, "cacheManager")) {
    throw __rhinoLocalFixtureFault("cache-manager", 
      "rhino-local: cacheManager." + method + ": no given.bindings.cacheManager"
    );
  }
}

cacheManager.exists = function (cacheName) {
  __rhinoLocalExpectArity("cacheManager.exists", arguments, 1);
  __rhinoLocalRequireCaches("exists");
  return false;
};

cacheManager.named = function (cacheName) {
  __rhinoLocalExpectArity("cacheManager.named", arguments, 1);
  __rhinoLocalRequireCaches("named");
  return null;
};

function __rhinoLocalJourneySeed(field) {
  var seed = __rhinoLocal.bindings.journey;
  if (!seed || seed[field] === undefined) {
    throw __rhinoLocalFixtureFault("journey", 
      "rhino-local: journey." + field + ": no given.bindings.journey." + field
    );
  }
  return seed[field];
}

journey.name = function () {
  __rhinoLocalExpectArity("journey.name", arguments, 0);
  return __rhinoLocalJourneySeed("name");
};

journey.innerJourney = function () {
  __rhinoLocalExpectArity("journey.innerJourney", arguments, 0);
  return __rhinoLocalJourneySeed("innerJourney");
};

journey.mustRun = function () {
  __rhinoLocalExpectArity("journey.mustRun", arguments, 0);
  return __rhinoLocalJourneySeed("mustRun");
};

journey.identityResource = function () {
  __rhinoLocalExpectArity("journey.identityResource", arguments, 0);
  return __rhinoLocalJourneySeed("identityResource");
};

// Capture the Rhino builtin before we shadow it. A function declaration
// would hoist and capture the wrapper itself.
var __rhinoLocalRealJavaImporter =
  typeof JavaImporter === "function" ? JavaImporter : null;

// A function, because AM hands back a Java class and `typeof` of one is
// "function" (measured 2026-10-01, live-java-importer-scope).
var __rhinoLocalJsonValueClass = function () {
  throw __rhinoLocalFault("json-value-constructor", "rhino-local: new JsonValue(...) is not modelled; use JsonValue.json or JsonValue.object");
};
var __rhinoLocalJsonValueStatics = {
  // A Java map (measured: put/get, prints `{ "a": "b" }`), so it shares the
  // registered map's model rather than keeping a second one.
  object: function () {
    return __rhinoLocalAsJavaMap({});
  },
  json: function (value) {
    return value;
  }
};
__rhinoLocalJsonValueClass.object = __rhinoLocalJsonValueStatics.object;
__rhinoLocalJsonValueClass.json = __rhinoLocalJsonValueStatics.json;

// Preserve native Java package branches used by other imports.
var __rhinoLocalRealOrg = typeof org === "undefined" ? null : org;
var __rhinoLocalOrgOverlay = {
  forgerock: {
    json: { JsonValue: __rhinoLocalJsonValueClass }
  }
};
if (__rhinoLocalRealOrg) {
  __rhinoLocalOrgOverlay.forgerock.openam = __rhinoLocalRealOrg.forgerock.openam;
  __rhinoLocalOrgOverlay.mozilla = __rhinoLocalRealOrg.mozilla;
}
var org = __rhinoLocalOrgOverlay;

// Next-gen hides `org.forgerock.openam.auth.node.api.Action` and
// `com.sun.identity.authentication.callbacks.HiddenValueCallback` from a
// script, even through an importer that names them (typeof "undefined",
// measured 2026-10-01, live-java-importer-scope). These two inert sentinels
// exist only so JavaImporter can recognise and swallow the names; neither is
// ever attached to an importer.
var __rhinoLocalActionClass = {};
var __rhinoLocalHiddenValueCallback = function () {};

// The two classes, reachable the way a script names them: as the class or its
// package. Neither class is on the local classpath, so on the JVM those
// expressions are Rhino package objects and are recognised by name; in the
// Node path the branches do not exist and are supplied here.
var __rhinoLocalActionPackage = { Action: __rhinoLocalActionClass };
var __rhinoLocalCallbacksPackage = {
  HiddenValueCallback: __rhinoLocalHiddenValueCallback
};
if (!__rhinoLocalOrgOverlay.forgerock.openam) {
  __rhinoLocalOrgOverlay.forgerock.openam = {
    auth: { node: { api: __rhinoLocalActionPackage } }
  };
}
if (typeof com === "undefined") {
  var com = {
    sun: {
      identity: { authentication: { callbacks: __rhinoLocalCallbacksPackage } }
    }
  };
}

function __rhinoLocalImportNames(arg, local, localPackage, className) {
  var name;
  if (arg === local || arg === localPackage) {
    return true;
  }
  if (arg === null || arg === undefined || typeof arg !== "object" && typeof arg !== "function") {
    return false;
  }
  try {
    name = String(arg);
  } catch (e) {
    return false;
  }
  return (
    name === "[JavaPackage " + className + "]" ||
    name === "[JavaPackage " + className.replace(/\.[^.]+$/, "") + "]"
  );
}

JavaImporter = function () {
  var real = null;
  var imports = [];
  var jsonValue = false;
  var i;
  // Each modelled class is reachable through its class or its package, and
  // through nothing else: JavaImporter() and JavaImporter(java.util) leave it
  // undefined (measured 2026-10-01 on next-gen, live-java-importer-scope).
  for (i = 0; i < arguments.length; i += 1) {
    if (
      arguments[i] === __rhinoLocalJsonValueClass ||
      arguments[i] === __rhinoLocalOrgOverlay.forgerock.json
    ) {
      jsonValue = true;
    } else if (
      __rhinoLocalImportNames(
        arguments[i],
        __rhinoLocalActionClass,
        __rhinoLocalActionPackage,
        "org.forgerock.openam.auth.node.api.Action"
      )
    ) {
      // Recognised and dropped: the class stays hidden.
    } else if (
      __rhinoLocalImportNames(
        arguments[i],
        __rhinoLocalHiddenValueCallback,
        __rhinoLocalCallbacksPackage,
        "com.sun.identity.authentication.callbacks.HiddenValueCallback"
      )
    ) {
      // Recognised and dropped, as above.
    } else {
      imports.push(arguments[i]);
    }
  }
  if (typeof __rhinoLocalRealJavaImporter === "function") {
    real = __rhinoLocalRealJavaImporter.apply(null, imports);
  }
  function Importer() {}
  if (real) {
    Importer.prototype = real;
  }
  var wrapper = new Importer();
  // Both classes stay undefined on the importer even when it names them
  // (measured 2026-10-01, live-java-importer-scope).
  if (jsonValue) {
    wrapper.JsonValue = __rhinoLocalJsonValueClass;
  }
  return wrapper;
};

// Opt-in phase timing (AIC_SCRIPT_TESTER_TIMING): the runner defines
// __rhinoLocalTimer for the prelude of a timed job only (java/PhaseTimer.java)
// and deletes it before the script runs. Untimed, this is null and a bracket
// costs one comparison.
var __rhinoLocalPhase = (function (timer) {
  if (timer === null) {
    return null;
  }
  var phase = {
    begin: function (name, label) {
      return timer("begin", name, label);
    },
    end: function (depth) {
      timer("end", depth);
    },
  };
  return typeof Object.freeze === "function" ? Object.freeze(phase) : phase;
})(typeof __rhinoLocalTimer === "function" ? __rhinoLocalTimer : null);

// What a required library cannot see, from any next-gen caller (docs/api/12,
// "Library scope when required", measured 2026-11-11). Reading one inside the
// library is the engine's ReferenceError; so is reading a local var of the
// caller. java/LibraryHost.java reads this list after the preamble; the
// LIBRARY overlay also unbinds it from its own (empty) caller.
var __rhinoLocalLibUnbound = [
  "accessToken", "action", "allowedScopes", "auditEntryDetail", "availableScopes",
  "cacheManager", "callbacks", "callbacksBuilder", "claims", "claimsLocales",
  "clientProperties", "config", "console", "defaultScopes", "existingSession",
  "idRepository", "identity", "journey", "locales", "nodeState", "oauthApplication",
  "outcome", "process", "properties", "rawProfile", "requestCookies", "requestHeaders",
  "requestParameters", "requestProperties", "requestedClaims", "requestedScopes",
  "resumedFromSuspend", "samlApplication", "scopeValidatorHelper", "scopes",
  "selectedIdp", "session", "sharedState", "token", "transientState"
];

// The runner defines __rhinoLocalLibraries for the preamble only
// (java/LibraryHost.java) and deletes it before the script runs: it answers a
// declared id with the module's loader, evaluated in a library scope (the
// bindings bound after the preamble, less the list above) from a compiled form
// shared across cases, so a library body is compiled once per JVM rather than
// once per case. A body a direct runner caller seeds in given.libraries is
// passed to it as source and compiled into the same library scope. Absent
// (the node vm unit-test path only), the bodies are eval'd here, in the
// caller's scope: that path does not model library scope.
var __rhinoLocalLoadLibrary = (function (host) {
  return function (name) {
    var id = String(name);
    if (__rhinoLocalHas(__rhinoLocal.requireCache, id)) {
      return __rhinoLocal.requireCache[id];
    }
    var seeded = __rhinoLocalHas(__rhinoLocal.libraries, id);
    var loader = host === null ? undefined : host(id);
    if (loader === undefined && !seeded) {
      throw __rhinoLocalFixtureFault("library", 
        "rhino-local: require: library " + JSON.stringify(id) +
          " is not declared on the suite (given.libraries); if this name exists on AIC, AM would load the tenant's copy and the lanes would disagree"
      );
    }
    var exports = {};
    var module = { exports: exports };
    __rhinoLocal.requireCache[id] = exports;
    if (loader === undefined && host !== null) {
      loader = host(id, String(__rhinoLocal.libraries[id]));
    }
    if (loader === undefined) {
      loader = eval(
        "(function (require, exports, module) {\n" + String(__rhinoLocal.libraries[id]) + "\n})"
      );
    }
    var timed = __rhinoLocalPhase === null ? null : __rhinoLocalPhase.begin("moduleBody", id);
    try {
      loader(require, exports, module);
    } finally {
      if (timed !== null) {
        __rhinoLocalPhase.end(timed);
      }
    }
    __rhinoLocal.requireCache[id] = module.exports;
    return module.exports;
  };
})(typeof __rhinoLocalLibraries === "function" ? __rhinoLocalLibraries : null);

// utils is backed by java/HostOps.java, a native function the runner defines
// for the preamble and deletes before the script runs: java.util.Base64,
// SecureRandom and javax.crypto are outside the AM allow-list, so neither the
// mocks nor a script can reach them directly. Absent (the node vm path), utils
// stays unmocked.
if (typeof __rhinoLocalHostOp !== "undefined") {
  (function (hostOp) {
    // AIC's service errors carry no (script#line) suffix; a JS-thrown
    // InternalError has none either, where one reported from Java would.
    function fail(message) {
      throw new InternalError(message);
    }
    // A number or boolean takes AIC's String overload ("MTI=" for 12).
    function bytes(value) {
      var t = typeof value;
      return t === "string" || t === "number" || t === "boolean"
        ? hostOp("utf8", String(value))
        : value;
    }
    function encoded(value, url) {
      return hostOp("base64Encode", bytes(value), url);
    }
    function decoded(value, url, asBytes) {
      var result;
      try {
        result = hostOp("base64Decode", String(value), url);
      } catch (e) {
        if (url) {
          return null;
        }
        if (typeof e === "string") {
          fail(e);
        }
        throw e;
      }
      return asBytes ? result : hostOp("fromUtf8", result);
    }
    [utils.base64, utils.base64url].forEach(function (codec, index) {
      var url = index === 1;
      codec.encode = function (value) {
        return encoded(value, url);
      };
      codec.decode = function (value) {
        return decoded(value, url, false);
      };
      codec.decodeToBytes = function (value) {
        return decoded(value, url, true);
      };
      codec.btoa = codec.encode;
      codec.atob = codec.decode;
    });
    utils.types.stringToBytes = function (value) {
      return hostOp("utf8", String(value));
    };
    utils.types.bytesToString = function (value) {
      return hostOp("fromUtf8", value);
    };
    utils.crypto.randomUUID = function () {
      __rhinoLocalExpectArity("utils.crypto.randomUUID", arguments, 0);
      return hostOp("uuid");
    };
    utils.crypto.getRandomValues = function (array) {
      __rhinoLocalExpectArity("utils.crypto.getRandomValues", arguments, 1);
      // AIC takes an array only; an array-like object has no overload.
      if (!Array.isArray(array) && !(array instanceof java.lang.Object)) {
        fail(
          "Can't find method org.forgerock.openam.scripting.bindings.crypto.ScriptCryptoService.getRandomValues(object)."
        );
      }
      var i;
      for (i = 0; i < array.length; i += 1) {
        array[i] = hostOp("randomInt32");
      }
      return array;
    };

    var subtle = utils.crypto.subtle;
    function algorithmName(value) {
      return typeof value === "string" ? value : value.name;
    }
    function hmacHash(algorithm) {
      var hash = typeof algorithm === "string" ? "SHA-256" : algorithm.hash;
      if (typeof hash !== "string") {
        fail("Unsupported hashing algorithm: [object Object]");
      }
      return hash;
    }
    // Only these names, and only as a string: { name } is refused (measured).
    var DIGESTS = ["SHA-256", "SHA-384", "SHA-1", "SHA-512"];
    subtle.digest = function (algorithm, data) {
      if (typeof algorithm !== "string" || DIGESTS.indexOf(algorithm) < 0) {
        fail("Algorithm must be one of [" + DIGESTS.join(", ") + "]");
      }
      return hostOp("digest", algorithm, data);
    };
    subtle.sign = function (algorithm, key, data) {
      var name = algorithmName(algorithm);
      if (name === "HMAC") {
        return hostOp("hmac", hmacHash(algorithm), key, data);
      }
      if (name === "ECDSA") {
        return hostOp("ecdsaSign", key, data);
      }
      fail("Algorithm must be one of [HMAC, ECDSA]");
    };
    subtle.verify = function (algorithm, key, data, signature) {
      var name = algorithmName(algorithm);
      if (name === "HMAC") {
        var expected = hostOp("hmac", hmacHash(algorithm), key, data);
        var i;
        if (expected.length !== signature.length) {
          return false;
        }
        for (i = 0; i < expected.length; i += 1) {
          if (expected[i] !== signature[i]) {
            return false;
          }
        }
        return true;
      }
      if (name === "ECDSA") {
        return hostOp("ecdsaVerify", key, data, signature);
      }
      fail("Algorithm must be one of [HMAC, ECDSA]");
    };
    function cipher(encrypt) {
      return function (algorithm, key, data) {
        var name = algorithmName(algorithm);
        if (name === "AES") {
          return hostOp("aes", encrypt, key, data);
        }
        if (name === "RSA") {
          return hostOp("rsa", encrypt, key, data);
        }
        fail("Algorithm must be one of [AES, RSA]");
      };
    }
    subtle.encrypt = cipher(true);
    subtle.decrypt = cipher(false);
    subtle.generateKey = function (algorithm) {
      var name = algorithmName(algorithm);
      if (name === "AES" || name === "HMAC") {
        return hostOp("generateSecret");
      }
      if (name === "RSA" || name === "ECDSA") {
        var pair = hostOp("generateKeyPair", name);
        // How AIC's key-pair map prints (measured).
        Object.defineProperty(pair, "toString", {
          enumerable: false,
          value: function () {
            return (
              '{ "privateKey": ' +
              this.privateKey +
              ', "publicKey": ' +
              this.publicKey +
              " }"
            );
          },
        });
        return pair;
      }
      fail("Algorithm must be one of [AES, ECDSA, RSA, HMAC]");
    };
    subtle.deriveKey = function (algorithm, baseKey, length) {
      if (typeof algorithm === "string") {
        fail("Salt must be provided for PBKDF2.");
      }
      if (algorithm.name !== "PBKDF2") {
        fail("Algorithm must be one of [PBKDF2]");
      }
      try {
        return hostOp(
          "pbkdf2",
          baseKey,
          algorithm.salt,
          algorithm.iterations,
          length,
          algorithm.hash
        );
      } catch (e) {
        if (typeof e === "string") {
          fail(e);
        }
        throw e;
      }
    };
  })(__rhinoLocalHostOp);
}

// Methods measured absent on AIC: calling one there is the engine's TypeError,
// so it is that here too and not a harness fault.
var __rhinoLocalMeasuredAbsent = {
  logger: ["message", "warning", "errorEnabled", "messageEnabled", "warningEnabled"],
  existingSession: ["hasOwnProperty"]
};

// Objects whose whole member set was captured on AIC. A name outside the set
// is absent there, so calling it is the engine's TypeError; a name inside it
// that this harness has not written is the fault. An object listed here needs
// no entry above.
var __rhinoLocalMeasuredMembers = {
  "idRepository.getIdentity()": [
    "addAttribute", "attribute", "attributeValues", "class", "equals", "exists",
    "getAttributeValues", "getClass", "getName", "getUniversalId", "hashCode", "name",
    "notify", "notifyAll", "setAttribute", "store", "toString", "universalId", "wait"
  ]
};

// A method a script calls on a harness object that this harness never wrote.
// AIC may well have it, so the call is not answered with the TypeError a
// missing method gets: it is a named fault, tallied in the run summary, and
// it fails the run even when the script catches it. The engine calls
// __noSuchMethod__ for a call to a member that is not a function, so one
// hidden member covers every name. It is installed on each service object
// the generated surface names, on the plain objects nested in them
// (utils.crypto.subtle), on the request maps, and on every object with
// methods of its own that a guarded object's method returns (an identity, an
// HTTP response), under the path of the call that made it. Not covered: a
// property read without a call (typeof openidm.nope), which the engine gives
// no hook for; an array; and a context overlay's own views.
function __rhinoLocalGuardService(path, object) {
  var names;
  var own;
  var i;
  if (object === null || typeof object !== "object" ||
      Object.prototype.toString.call(object) !== "[object Object]" ||
      Object.prototype.hasOwnProperty.call(object, "__noSuchMethod__")) {
    return object;
  }
  __rhinoLocalHide(object, "__noSuchMethod__", function (name) {
    var members = __rhinoLocalMeasuredMembers[path];
    if (members ? members.indexOf(name) === -1
        : (__rhinoLocalMeasuredAbsent[path] || []).indexOf(name) !== -1) {
      throw new TypeError("Cannot find function " + name + " in object " + String(object) + ".");
    }
    if (name in object) {
      // A member that is there and is not a function (a map entry named like
      // a method): the engine's own refusal.
      throw new TypeError("Cannot call property " + name + " in object " + String(object) +
        ". It is not a function, it is \"" + typeof object[name] + "\".");
    }
    throw __rhinoLocalFault(
      "not-implemented/" + path + "." + name,
      "rhino-local: not implemented: " + path + "." + name +
        "() is not a member this harness has; if AIC has it, it needs adding");
  });
  names = Object.getOwnPropertyNames(object);
  for (i = 0; i < names.length; i += 1) {
    own = Object.getOwnPropertyDescriptor(object, names[i]);
    if (own === undefined || !("value" in own) || names[i] === "__noSuchMethod__") {
      continue;
    }
    if (typeof own.value === "function") {
      if (own.writable || own.configurable) {
        own.value = __rhinoLocalGuardReturns(path + "." + names[i] + "()", own.value);
        Object.defineProperty(object, names[i], own);
      }
    } else if (own.enumerable) {
      __rhinoLocalGuardService(path + "." + names[i], own.value);
    }
  }
  return object;
}

// The method, with whatever harness object it returns guarded too. Only an
// object carrying methods of its own is one: a record or a parsed body is
// plain data, where a missing method is the script's own TypeError.
function __rhinoLocalGuardReturns(path, method) {
  if (Object.keys(method).length > 0 ||
      (method.prototype && Object.getOwnPropertyNames(method.prototype).length > 1)) {
    return method;
  }
  return function () {
    var result = method.apply(this, arguments);
    var names;
    var i;
    if (result === null || typeof result !== "object" ||
        Object.prototype.toString.call(result) !== "[object Object]") {
      return result;
    }
    names = Object.getOwnPropertyNames(result);
    for (i = 0; i < names.length; i += 1) {
      if (typeof Object.getOwnPropertyDescriptor(result, names[i]).value === "function") {
        return __rhinoLocalGuardService(path, result);
      }
    }
    return result;
  };
}

function __rhinoLocalGuardServices() {
  var services = typeof __rhinoLocalServices === "function" ? __rhinoLocalServices() : {};
  var names = Object.keys(services);
  var i;
  for (i = 0; i < names.length; i += 1) {
    __rhinoLocalGuardService(names[i], services[names[i]]);
  }
}

function __rhinoLocalSeed(given) {
  // Defined by the runner for the preamble only; captured first, so a fault
  // raised while seeding (seed-binding, seed-cache-manager) is recorded too.
  __rhinoLocalFaultSink.capture();
  given = given || {};
  __rhinoLocal.loggerScriptId = given.loggerScriptId || "<unseeded-script-id>";
  __rhinoLocal.bindings = __rhinoLocalClone(given.bindings || {});
  var seeded = Object.keys(__rhinoLocal.bindings);
  var s;
  var journeyFields = ["name", "identityResource", "innerJourney", "mustRun"];
  for (s = 0; s < seeded.length; s += 1) {
    if (seeded[s] === "journey") {
      Object.keys(__rhinoLocal.bindings.journey || {}).forEach(function (f) {
        if (journeyFields.indexOf(f) === -1) {
          throw __rhinoLocalFail(
            "rhino-local: given.bindings.journey." + f + " is not a journey seed"
          );
        }
      });
    } else if (seeded[s] === "cacheManager") {
      if (Object.keys(__rhinoLocal.bindings.cacheManager || {}).length > 0) {
        throw __rhinoLocalFault("seed-cache-manager", 
          "rhino-local: given.bindings.cacheManager must be {}; seeding caches is not implemented"
        );
      }
    } else {
      throw __rhinoLocalFault("seed-binding", 
        "rhino-local: given.bindings." +
          seeded[s] +
          " seeding is not implemented; only journey and cacheManager are"
      );
    }
  }
  // Measured null in a journey: every call is "Cannot call method … of null".
  samlApplication = null;
  oauthApplication = null;
  __rhinoLocal.shared = __rhinoLocalClone(given.sharedState || {});
  __rhinoLocal.transient = __rhinoLocalClone(given.transientState || {});
  __rhinoLocal.secure = __rhinoLocalClone(given.secureState || {});
  if (given.registeredObjectAttributes !== undefined) {
    __rhinoLocalMergeBucket("shared", {
      objectAttributes: __rhinoLocalClone(given.registeredObjectAttributes)
    });
  }
  __rhinoLocal.initialShared = __rhinoLocalClone(__rhinoLocal.shared);
  __rhinoLocal.initialTransient = __rhinoLocalClone(__rhinoLocal.transient);
  __rhinoLocal.initialSecure = __rhinoLocalClone(__rhinoLocal.secure);
  __rhinoLocal.sessionProperties = __rhinoLocalClone(given.existingSession || {});
  __rhinoLocal.initialSessionProperties = __rhinoLocalClone(__rhinoLocal.sessionProperties);
  __rhinoLocal.managed = __rhinoLocalClone(given.managed || {});
  __rhinoLocal.identityCustomAttrs = __rhinoLocalClone(given.identityCustomAttrs || {});
  __rhinoLocal.identityCustomAttrsOwnedKeys = __rhinoLocalClone(given.identityCustomAttrsOwnedKeys || {});
  __rhinoLocal.identityGenerations = {};
  // Set of `managed/<name>` this environment declares. Membership only — every
  // schema RULE (properties, required, enum) is checked in the Node layer, so
  // there is exactly one implementation of each.
  __rhinoLocal.knownTypes = given.knownTypes
    ? __rhinoLocalClone(given.knownTypes)
    : null;
  __rhinoLocal.profileTenant = given.profileTenant || null;
  __rhinoLocal.profilePulledAt = given.profilePulledAt || null;
  // `managed/<type>` → method → { id, message }: the writes this lane cannot
  // answer because the type declares hooks or policies (E15b). Decided in
  // src/profile/write-guard.ts; this side only looks the answer up.
  __rhinoLocal.managedWriteFaults = given.managedWriteFaults
    ? __rhinoLocalClone(given.managedWriteFaults)
    : null;
  // `managed/<type>` → { defaults, notNull, nullRefused, emptyOnNull,
  // nullUnmeasured, checked, mismatchUnmeasured, itemTypes, defaulted }: the type's implicit policies (P2-2,
  // sol-r1b), decided in src/profile/write-guard.ts managedPolicy.
  __rhinoLocal.managedPolicy = given.managedPolicy
    ? __rhinoLocalClone(given.managedPolicy)
    : null;
  // { id, message } or null: the fault every managed call raises when the
  // profile declares an invalid default (sol-r1c), from write-guard.ts
  // managedInvalidDefault.
  __rhinoLocal.managedInvalidDefault = given.managedInvalidDefault
    ? __rhinoLocalClone(given.managedInvalidDefault)
    : null;
  __rhinoLocal.esv = __rhinoLocalClone(given.esv || {});
  __rhinoLocal.esvProvided = given.esv !== undefined;
  __rhinoLocal.esvUndeclared = given.esvUndeclared || "error";
  __rhinoLocal.secrets = __rhinoLocalClone(given.secrets || {});
  __rhinoLocal.secretsProvided = given.secrets !== undefined;
  __rhinoLocal.libraries = given.libraries
    ? __rhinoLocalClone(given.libraries)
    : {};
  __rhinoLocal.requireCache = {};
  require = function (name) {
    __rhinoLocalExpectArity("require", arguments, 1);
    return __rhinoLocalLoadLibrary(name);
  };
  if (given.callbacks === undefined) {
    __rhinoLocal.submittedCallbacks = null;
  } else {
    __rhinoLocal.submittedCallbacks = __rhinoLocalClone(given.callbacks);
  }
  __rhinoLocal.httpStubs = given.http ? __rhinoLocalClone(given.http) : [];
  __rhinoLocal.openidmActions = given.openidmActions ? __rhinoLocalClone(given.openidmActions) : [];
  __rhinoLocal.openidmFailures = given.openidmFailures
    ? __rhinoLocalClone(given.openidmFailures)
    : [];
  __rhinoLocal.openidmCallCounts = __rhinoLocalClone(given.openidmPriorCalls || {});
  __rhinoLocal.generatedId = 0;
  __rhinoLocal.outcome = null;
  __rhinoLocal.callbacks = [];
  __rhinoLocal.openidm = [];
  __rhinoLocal.http = [];
  __rhinoLocal.logs = [];
  __rhinoLocal.identityWrites = [];
  __rhinoLocal.faults = [];
  // Local-only: the AIC lane runs against the tenant's real mapping.
  __rhinoLocal.identityAttributes = __rhinoLocalClone(given.identityAttributes || {});

  requestHeaders = __rhinoLocalRequestMap(given.requestHeaders || {}, {
    caseInsensitive: true,
    asList: true,
    label: "requestHeaders",
  });
  requestParameters = __rhinoLocalRequestMap(given.requestParameters || {}, {
    caseInsensitive: false,
    asList: true,
    label: "requestParameters",
  });

  // Assigned either way: the runner's scope outlives one case, so leaving a
  // previous case's session installed would seed a binding nobody declared.
  existingSession =
    given.existingSession === undefined
      ? undefined
      : __rhinoLocalSessionMap(given.existingSession);

  if (given.realm !== undefined) {
    realm = given.realm;
  }
  if (given.scriptName !== undefined) {
    scriptName = given.scriptName;
  }
  if (given.cookieName !== undefined) {
    cookieName = given.cookieName;
  }
  // A request that carries a session carries its cookie, and AM lists it in
  // requestCookies as a string beside the author's cookies (measured
  // 2026-10-01, live-session-cookie). The token is per-run on the tenant, so
  // the local value is a fixed placeholder: presence, count and type agree,
  // the value never can. An author cookie of the same name is refused, as
  // the AIC lane's attachSessionCookie refuses it.
  var cookies = __rhinoLocalClone(given.requestCookies || {});
  if (given.existingSession !== undefined) {
    if (Object.prototype.hasOwnProperty.call(cookies, String(cookieName))) {
      throw __rhinoLocalFail(
        "rhino-local: request cookie " +
          JSON.stringify(String(cookieName)) +
          " collides with the tenant session cookie"
      );
    }
    cookies[String(cookieName)] = "rhino-local-session-token";
  }
  requestCookies = __rhinoLocalRequestMap(cookies, {
    caseInsensitive: false,
    asList: false,
    label: "requestCookies",
  });
  if (given.resumedFromSuspend !== undefined) {
    resumedFromSuspend = given.resumedFromSuspend;
  }
  if (given.locales !== undefined) {
    locales = given.locales;
  }
}

function __rhinoLocalHarvest() {
  if (__rhinoLocalPhase === null) {
    return __rhinoLocalHarvestNow();
  }
  var timed = __rhinoLocalPhase.begin("harvest", null);
  try {
    return __rhinoLocalHarvestNow();
  } finally {
    __rhinoLocalPhase.end(timed);
  }
}

function __rhinoLocalHarvestNow() {
  if (__rhinoLocal.faults.length > 0) {
    // The wording answers for every fault, not only the first one named
    // (round-3 P3-5): a case can hit a gap and a script defect together.
    var distinct = __rhinoLocal.faults.filter(function (fault, index, all) {
      return all.indexOf(fault) === index;
    });
    var defects = distinct.filter(function (fault) {
      return fault.indexOf("script-defect/") === 0;
    }).length;
    var gaps = distinct.length - defects;
    throw new Error(
      "rhino-local harness fault " + __rhinoLocal.faults[0] +
        (gaps === 0
          ? " (a script defect: AIC's behaviour here is measured and is not what the script means, so the run fails even though the script caught it)"
          : defects === 0
            ? " (the script caught this refusal, but the harness cannot say what AIC does there, so the run fails anyway)"
            : " (and " + (distinct.length - 1) + " more: " + defects +
              " script defect(s), where AIC's behaviour is measured and is not what the script means, and " + gaps +
              " behaviour(s) the harness cannot say AIC's answer for; the run fails even though the script caught them)")
    );
  }
  var recordedOutcome = __rhinoLocal.outcome;
  if (
    recordedOutcome === null &&
    typeof outcome !== "undefined" &&
    outcome !== null
  ) {
    recordedOutcome = String(outcome);
  }
  // Queued next-gen callbacks suspend the node even after goTo. Preserve the
  // discarded decision as diagnostic evidence without judging it as an effect.
  var discardedOutcome = null;
  if (__rhinoLocal.callbacks.length > 0) {
    discardedOutcome = recordedOutcome;
    recordedOutcome = null;
  }
  return JSON.stringify({
    outcome: recordedOutcome,
    discardedOutcome: discardedOutcome,
    sharedState: {
      initial: __rhinoLocal.initialShared,
      final: __rhinoLocalClone(__rhinoLocal.shared),
    },
    transientState: {
      initial: __rhinoLocal.initialTransient,
      final: __rhinoLocalClone(__rhinoLocal.transient),
    },
    secureState: {
      initial: __rhinoLocal.initialSecure,
      final: __rhinoLocalClone(__rhinoLocal.secure),
    },
    sessionProperties: {
      initial: __rhinoLocal.initialSessionProperties,
      final: __rhinoLocalClone(__rhinoLocal.sessionProperties),
    },
    callbacks: __rhinoLocal.callbacks,
    openidm: __rhinoLocal.openidm,
    managedStore: __rhinoLocalClone(__rhinoLocal.managed),
    identityCustomAttrs: __rhinoLocalClone(__rhinoLocal.identityCustomAttrs),
    identityCustomAttrsOwnedKeys: __rhinoLocalClone(__rhinoLocal.identityCustomAttrsOwnedKeys),
    http: __rhinoLocal.http,
    logs: __rhinoLocal.logs,
    identityWrites: __rhinoLocal.identityWrites,
  });
}
