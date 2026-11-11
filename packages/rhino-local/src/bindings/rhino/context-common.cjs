// Shared view helpers for the non-decision script contexts (OAuth2 access
// token modification, OIDC claims, ...). Appended to the prelude after
// runtime.cjs and before the context's own overlay, and only when a case names
// such a context, so a decision-node case's prelude is byte-for-byte what it
// was. Same Rhino 1.7.14 restrictions as runtime.cjs: no let, no top-level
// const, no shorthand, no Map/Set/Symbol/Proxy.
//
// The rule every helper here keeps: a behaviour is emulated only where a live
// capture measured it. Everything else is a named, tallied harness fault,
// `<prefix>/<binding>.<member>`, where the context's overlay sets `prefix`
// (for example `oauth2-token-unmeasured`). A catchable ordinary error that AIC
// was never seen to throw would let a script's try/catch turn a guess into a
// pass, so the helpers never invent one.

// The top-level scope, so a context seed can remove a global the context does
// not bind: a script that reads one then gets AIC's ReferenceError.
var __rhinoLocalTop = this;

// java/HostOps.java, defined by the runner for the prelude only. Its
// guard ops wrap a real Java object so that only measured members answer
// (java/ContextJavaGuard.java).
var __rhinoLocalCtxHostOp = typeof __rhinoLocalHostOp !== "undefined" ? __rhinoLocalHostOp : null;

// A real Java object behind a member gate: `admitted` is {name: argument
// count or [counts]}. Every other Java member faults.
function __rhinoLocalCtxGuardMap(map, admitted, label) {
  return __rhinoLocalCtxHostOp("guardMap", map, admitted, label, __rhinoLocalCtxUnmeasured);
}

// The bare Java object behind a guard, for the harness's own reads.
function __rhinoLocalCtxUnguard(value) {
  return __rhinoLocalCtxHostOp === null ? value : __rhinoLocalCtxHostOp("unguard", value);
}

// Set by the context overlay (prefix, noun, listKind) and its seed (engine).
// `views` holds the binding objects whose own properties the harvest audits.
var __rhinoLocalCtx = {
  engine: "next-gen",
  prefix: "context-unmeasured",
  noun: "script",
  listKind: null,
  views: [],
  sealed: true,
  realJavaStrings: false,
};

function __rhinoLocalCtxUnmeasured(member, detail) {
  return __rhinoLocalFault(__rhinoLocalCtx.prefix + "/" + member,
    "rhino-local: " + member + (detail ? " " + detail : "") +
      " in the " + __rhinoLocalCtx.noun + " context (" + __rhinoLocalCtx.engine +
      ") is unmeasured; docs/api/12-script-bindings-matrix.md lists what was measured");
}

function __rhinoLocalCtxNoMethod(className, name, args) {
  return __rhinoLocalInternalError("Can't find method " + className + "." + name + "(" +
    __rhinoLocalScriptSignature(args) + ").");
}

// Install a method that answers only at its measured argument count. Any
// other count is a named fault, because which overloads a Java method has is
// unknown here. `measuredRefusal` true admits Rhino's ordinary overload error
// for every wrong count, which is what accessToken setField/getField do
// (2026-11-10). May-act's token methods do not use this helper: a refused
// count there is admitted only for the counts the captures record.
function __rhinoLocalCtxMethod(target, label, className, name, arity, fn, measuredRefusal) {
  __rhinoLocalHide(target, name, function () {
    if (arguments.length !== arity) {
      if (measuredRefusal) {
        throw __rhinoLocalCtxNoMethod(className, name, arguments);
      }
      throw __rhinoLocalCtxUnmeasured(label + "." + name + "(" + __rhinoLocalArgShape(arguments) + ")",
        "with " + arguments.length + " argument(s); only " + arity + " was measured");
    }
    return fn.apply(this, arguments);
  });
}

function __rhinoLocalCtxFaulting(target, label, names) {
  var i;
  for (i = 0; i < names.length; i += 1) {
    (function (name) {
      __rhinoLocalHide(target, name, function () {
        throw __rhinoLocalCtxUnmeasured(label + "." + name + "()");
      });
    })(names[i]);
  }
}

var __rhinoLocalCtxObjectMethods = ["toString", "getClass", "hashCode", "equals", "wait", "notify", "notifyAll"];

function __rhinoLocalCtxToJSON(target, label) {
  __rhinoLocalHide(target, "toJSON", function () {
    throw __rhinoLocalCtxUnmeasured("JSON.stringify(" + label + ")");
  });
}

// --- Object.prototype.toString tags -----------------------------------------
//
// A Java object's tag on AIC is `[object JavaObject]` (measured for a legacy
// JsonValue, 2026-11-10). A view carries its tag as a hidden own data property:
// a string where the tag was measured, `{ label }` where it was not, which
// faults. Objects without the key reach the native toString untouched.
var __rhinoLocalTagKey = "__rhinoLocalTag";

function __rhinoLocalCtxTag(target, tag, label) {
  __rhinoLocalHide(target, __rhinoLocalTagKey, tag === null ? { label: label } : tag);
}

(function () {
  "use strict";
  var original = Object.prototype.toString;
  __rhinoLocalHide(Object.prototype, "toString", function toString() {
    if (this !== null && this !== undefined && typeof this === "object") {
      var own;
      try {
        own = Object.getOwnPropertyDescriptor(this, __rhinoLocalTagKey);
      } catch (e) {
        own = undefined;
      }
      if (own !== undefined && "value" in own) {
        if (typeof own.value === "string") {
          return "[object " + own.value + "]";
        }
        if (own.value !== null && typeof own.value === "object" && typeof own.value.label === "string") {
          throw __rhinoLocalCtxUnmeasured("Object.prototype.toString.call(" + own.value.label + ")",
            "(the Java object's tag)");
        }
      }
    }
    return original.call(this);
  });
})();

// --- Generic Array.prototype receivers ----------------------------------------
//
// `Array.prototype.map.call(list, fn)` works on any array-like, so a list view
// would answer it as a JS array would, where AIC hands over a Java list whose
// behaviour as such a receiver is unmeasured. Each Array.prototype method is
// wrapped thinly: a real array takes the native path first and unchanged, any
// other receiver too, unless it is a list view whose kind says
// `genericReceiver: false`, which faults. concat keeps runtime.cjs's wrapper.
(function () {
  "use strict";
  var names = Object.getOwnPropertyNames(Array.prototype);
  var i;
  for (i = 0; i < names.length; i += 1) {
    (function (name) {
      if (name === "constructor" || name === "concat" || name === "length") {
        return;
      }
      var own = Object.getOwnPropertyDescriptor(Array.prototype, name);
      if (own === undefined || typeof own.value !== "function") {
        return;
      }
      var original = own.value;
      __rhinoLocalHide(Array.prototype, name, function () {
        if (!Array.isArray(this)) {
          var row = __rhinoLocalListRow(this);
          // `genericReceiver` may instead list the method names its context
          // measured (validate scope: slice); every other name faults.
          var allowed = row === null ? true : row.kind.genericReceiver;
          if (row !== null && (allowed === false || (Array.isArray(allowed) && allowed.indexOf(name) === -1))) {
            throw __rhinoLocalListUnmeasured(row.kind, "Array.prototype." + name + ".call(v, …)");
          }
        }
        return original.apply(this, arguments);
      });
    })(names[i]);
  }
})();

// --- Mutation audit ----------------------------------------------------------
//
// Rhino 1.7.14 has no Proxy, so adding a key to a view, deleting one, or
// replacing a hidden method cannot be intercepted as it happens. Each binding
// view is snapshotted when built, and the harvest compares: any difference is
// a recorded fault naming the view and the key. Writes to a known entry fault
// at once (its accessor has a faulting setter).
function __rhinoLocalCtxSnapshot(target) {
  var names = Object.getOwnPropertyNames(target);
  var snap = {};
  var i;
  var own;
  for (i = 0; i < names.length; i += 1) {
    own = Object.getOwnPropertyDescriptor(target, names[i]);
    Object.defineProperty(snap, names[i], {
      value: "value" in own ? { value: own.value } : { get: own.get, set: own.set },
      enumerable: true, configurable: true, writable: true,
    });
  }
  return snap;
}

// While a context seed is still installing measured members on a view, its
// snapshot waits for __rhinoLocalCtxSeal, so those members are part of the
// baseline rather than additions. A view made after the seed is snapshotted
// at once.
function __rhinoLocalCtxWatch(target, label) {
  var i;
  for (i = 0; i < __rhinoLocalCtx.views.length; i += 1) {
    if (__rhinoLocalCtx.views[i].target === target) {
      __rhinoLocalCtx.views[i].label = label;
      return target;
    }
  }
  __rhinoLocalCtx.views.push({
    target: target, label: label, snap: __rhinoLocalCtx.sealed ? __rhinoLocalCtxSnapshot(target) : null,
  });
  return target;
}

function __rhinoLocalCtxSeal() {
  var i;
  for (i = 0; i < __rhinoLocalCtx.views.length; i += 1) {
    if (__rhinoLocalCtx.views[i].snap === null) {
      __rhinoLocalCtx.views[i].snap = __rhinoLocalCtxSnapshot(__rhinoLocalCtx.views[i].target);
    }
  }
  __rhinoLocalCtx.sealed = true;
}

// A measured mutation (token-context scopes.add()) moves the baseline.
function __rhinoLocalCtxRewatch(target) {
  var i;
  for (i = 0; i < __rhinoLocalCtx.views.length; i += 1) {
    if (__rhinoLocalCtx.views[i].target === target) {
      __rhinoLocalCtx.views[i].snap = __rhinoLocalCtxSnapshot(target);
    }
  }
}

// Compare one view with its snapshot; every difference is a recorded fault.
function __rhinoLocalCtxAuditView(view) {
  var i;
  var names;
  var own;
  var was;
  var seen;
  if (view.snap === null) {
    throw new Error("rhino-local: the context seed never sealed its views (__rhinoLocalCtxSeal)");
  }
  names = Object.getOwnPropertyNames(view.target);
  seen = {};
  for (i = 0; i < names.length; i += 1) {
    Object.defineProperty(seen, names[i], { value: true, enumerable: true, configurable: true });
    own = Object.getOwnPropertyDescriptor(view.target, names[i]);
    was = Object.prototype.hasOwnProperty.call(view.snap, names[i]) ? view.snap[names[i]] : null;
    if (was === null) {
      __rhinoLocalCtxUnmeasured(view.label + "." + names[i], "(adding a key to the Java object)");
    } else if ("value" in own ? !("value" in was) || own.value !== was.value : own.get !== was.get || own.set !== was.set) {
      __rhinoLocalCtxUnmeasured(view.label + "." + names[i], "(assigning or redefining a member of the Java object)");
    }
  }
  names = Object.keys(view.snap);
  for (i = 0; i < names.length; i += 1) {
    if (!Object.prototype.hasOwnProperty.call(seen, names[i])) {
      __rhinoLocalCtxUnmeasured(view.label + "." + names[i], "(deleting a member of the Java object)");
    }
  }
}

// Before a measured mutation moves a view's baseline (__rhinoLocalCtxRewatch),
// audit what changed since the last snapshot, so an earlier unmeasured write
// (`scopes[0] = "x"`) is recorded rather than adopted into the new baseline
// (Sol ctx r3 P2-1).
function __rhinoLocalCtxCheck(target) {
  var i;
  for (i = 0; i < __rhinoLocalCtx.views.length; i += 1) {
    if (__rhinoLocalCtx.views[i].target === target && __rhinoLocalCtx.views[i].snap !== null) {
      __rhinoLocalCtxAuditView(__rhinoLocalCtx.views[i]);
    }
  }
}

function __rhinoLocalCtxAudit() {
  var v;
  for (v = 0; v < __rhinoLocalCtx.views.length; v += 1) {
    __rhinoLocalCtxAuditView(__rhinoLocalCtx.views[v]);
  }
}

// --- Value views -------------------------------------------------------------

function __rhinoLocalCtxBare() {
  return typeof Object.create === "function" ? Object.create(null) : {};
}

// A java.lang.String as the legacy engine hands it over: typeof "object",
// String(v) and `"" + v` give the text, `v == "x"` is true and `v === "x"` is
// false, and `length` is a method. Java's other String methods are unmeasured.
//
// A context whose own bindings hand over real Java strings sets
// `__rhinoLocalCtx.realJavaStrings` (OIDC claims: `claims` is a real
// java.util.LinkedHashMap, so its values are real java.lang.String objects,
// and `claims.sub == identity.getName()` was measured true, which two views,
// or a view and a real string, cannot reproduce). Every legacy string is then
// a real java.lang.String, which behaves as it does on AIC's JVM, behind
// java/ContextJavaGuard.java's gate: only `length` and toString() of its own
// members answer (Sol ctx r2 P2-6), as on the view.
function __rhinoLocalCtxJavaString(text) {
  if (text === null || text === undefined) {
    return null;
  }
  if (__rhinoLocalCtx.realJavaStrings) {
    return __rhinoLocalCtxHostOp("guardString", String(text), "java.lang.String", __rhinoLocalCtxUnmeasured);
  }
  var value = String(text);
  var out = __rhinoLocalCtxBare();
  __rhinoLocalHide(out, "toString", function () {
    return value;
  });
  __rhinoLocalHide(out, "valueOf", function () {
    return value;
  });
  __rhinoLocalHide(out, "length", function () {
    return value.length;
  });
  __rhinoLocalCtxFaulting(out, "java.lang.String", [
    "charAt", "codePointAt", "compareTo", "compareToIgnoreCase", "concat", "contains", "contentEquals", "endsWith",
    "equals", "equalsIgnoreCase", "getBytes", "getClass", "hashCode", "indexOf", "intern", "isBlank", "isEmpty",
    "lastIndexOf", "lines", "matches", "repeat", "replace", "replaceAll", "replaceFirst", "split", "startsWith",
    "strip", "substring", "toCharArray", "toLowerCase", "toUpperCase", "trim", "slice", "match", "search",
  ]);
  __rhinoLocalCtxToJSON(out, "a Java string");
  __rhinoLocalCtxTag(out, null, "a Java string");
  return out;
}

// A legacy java.lang.Integer: typeof "object", String(v) gives the digits.
function __rhinoLocalCtxJavaNumber(n) {
  var out = __rhinoLocalCtxBare();
  __rhinoLocalHide(out, "toString", function () {
    return String(n);
  });
  __rhinoLocalHide(out, "valueOf", function () {
    return n;
  });
  __rhinoLocalCtxFaulting(out, "java.lang.Integer", [
    "intValue", "longValue", "doubleValue", "equals", "compareTo", "hashCode", "getClass",
  ]);
  __rhinoLocalCtxToJSON(out, "a Java number");
  __rhinoLocalCtxTag(out, null, "a Java number");
  return out;
}

function __rhinoLocalCtxIsEmpty(value) {
  if (Array.isArray(value)) {
    return value.length === 0;
  }
  return Object.keys(value).length === 0;
}

// The JsonValue a legacy getField / getAct hands back: typeof "object",
// member reads such as `v.a` are undefined, String(v) is the JSON-ish
// rendering (`"hello"`, `42.0`, `{ "a": 1.0 }`), and the
// Object.prototype.toString tag is `[object JavaObject]` (all measured
// 2026-11-10). size() was measured only on a null value (0).
function __rhinoLocalCtxJsonValue(value) {
  var out = __rhinoLocalCtxBare();
  __rhinoLocalHide(out, "toString", function () {
    if (value !== null && typeof value === "object" && __rhinoLocalCtxIsEmpty(value)) {
      throw __rhinoLocalCtxUnmeasured("String(JsonValue)", "of an empty object or array");
    }
    return __rhinoLocalJavaMapString(value);
  });
  __rhinoLocalHide(out, "size", function () {
    if (value === null && arguments.length === 0) {
      return 0;
    }
    throw __rhinoLocalCtxUnmeasured("JsonValue.size()", "on a non-null value");
  });
  __rhinoLocalCtxFaulting(out, "JsonValue", [
    "get", "getObject", "isNull", "isString", "asString", "isMap", "asMap", "isList", "asList", "isNumber",
    "asNumber", "asInteger", "asLong", "asDouble", "isBoolean", "asBoolean", "put", "add", "remove", "keys",
    "expect", "required", "defaultTo", "getPointer", "copy", "clone", "isDefined", "contains", "iterator",
    "getClass", "hashCode", "equals",
  ]);
  __rhinoLocalCtxToJSON(out, "a JsonValue");
  __rhinoLocalCtxTag(out, "JavaObject", "a JsonValue");
  return out;
}

// The engine's string: a JS string on next-gen, a Java string on legacy.
function __rhinoLocalCtxString(text) {
  if (__rhinoLocalCtx.engine === "legacy") {
    return __rhinoLocalCtxJavaString(text);
  }
  return text;
}

// A Java list, through runtime.cjs's list view. `kind` omitted uses the
// context's own kind; a caller passes one when this list was measured
// separately from the context's scope lists (Sol ctx r5 P2-6). `grant`
// omitted leaves the kind's empty policy; a caller passes one only for the
// binding a capture measured empty (Sol ctx r11 P2-1). Watched like the
// other views (Sol ctx r2 P2-3): an index write or a replaced method is
// found by the harvest audit.
function __rhinoLocalCtxList(items, label, kind, grant) {
  var list = __rhinoLocalListView(items, kind || __rhinoLocalCtx.listKind, grant);
  __rhinoLocalCtxTag(list, null, label || "a Java list");
  return __rhinoLocalCtxWatch(list, label || "list");
}

// A legacy java.util set: no `length`, `[i]` is AIC's InternalError
// (measured for index 0; the rest of the range assumed the same), and only
// size(), contains(), isEmpty(), toArray() and String(v) were measured, plus
// add() where the caller says so. Duplicate members collapse, as a set does.
// A HashSet or LinkedHashSet keeps first-seen order: the capture's HashSet
// happened to iterate in seed order, and JVM hash order is not reproduced.
// A TreeSet is ordered by Java's string order (UTF-16, the same order as
// Array.prototype.sort on these strings) on the seed and on add() (Sol ctx
// r5 P2-1). The capture's TreeSet values were already sorted, so that order
// is the set's measured type, not a value the probe distinguished.
// `javaClass` names the class in the `[i]` message when it is another set
// (validate scope's TreeSet, measured 2026-11-10).
// `tag` is the measured Object.prototype.toString tag, set before the watch.
// Absent, the tag stays unmeasured and faults.
function __rhinoLocalCtxJavaSet(items, label, addable, javaClass, tag) {
  var setClass = javaClass || "java.util.HashSet";
  var sorted = setClass === "java.util.TreeSet";
  var out = __rhinoLocalCtxBare();
  var store = [];
  var i;
  var item;
  for (i = 0; i < items.length; i += 1) {
    item = items[i];
    if (store.indexOf(item) === -1) {
      store.push(item);
    }
  }
  if (sorted) {
    store.sort();
  }
  for (i = 0; i < store.length + 8; i += 1) {
    (function (index) {
      Object.defineProperty(out, String(index), {
        enumerable: false,
        configurable: true,
        get: function () {
          throw __rhinoLocalInternalError('Java class "' + setClass + '" has no public instance field or method named "' +
            index + '".');
        },
        set: function () {
          throw __rhinoLocalCtxUnmeasured(label + "[" + index + "]", "assignment");
        },
      });
    })(i);
  }
  __rhinoLocalCtxFaulting(out, label, [
    "addAll", "clear", "containsAll", "equals", "forEach", "getClass", "hashCode", "iterator",
    "parallelStream", "remove", "removeAll", "removeIf", "retainAll", "spliterator", "stream", "clone",
  ]);
  __rhinoLocalCtxMethod(out, label, setClass, "isEmpty", 0, function () {
    return store.length === 0;
  });
  // Measured 2026-09-10 on identity.getAttribute(): toArray() resolves, and
  // String(v.toArray()[0]) is how a single-valued attribute is read.
  __rhinoLocalCtxMethod(out, label, setClass, "toArray", 0, function () {
    var arr = [];
    var k;
    for (k = 0; k < store.length; k += 1) {
      arr.push(__rhinoLocalCtxJavaString(store[k]));
    }
    return arr;
  });
  __rhinoLocalCtxMethod(out, label, setClass, "size", 0, function () {
    return store.length;
  });
  __rhinoLocalCtxMethod(out, label, setClass, "contains", 1, function (v) {
    return store.indexOf(String(v)) !== -1;
  });
  __rhinoLocalHide(out, "add", function (v) {
    if (!addable) {
      throw __rhinoLocalCtxUnmeasured(label + ".add()");
    }
    if (arguments.length !== 1 || typeof v !== "string") {
      throw __rhinoLocalCtxUnmeasured(label + ".add(" + __rhinoLocalArgShape(arguments) + ")");
    }
    if (store.indexOf(v) !== -1) {
      return false;
    }
    if (sorted) {
      var at = store.length;
      var j;
      for (j = 0; j < store.length; j += 1) {
        if (v < store[j]) {
          at = j;
          break;
        }
      }
      store.splice(at, 0, v);
    } else {
      store.push(v);
    }
    return true;
  });
  __rhinoLocalHide(out, "toString", function () {
    return "[" + store.join(", ") + "]";
  });
  __rhinoLocalCtxToJSON(out, label);
  __rhinoLocalCtxTag(out, tag === undefined ? null : tag, label);
  return __rhinoLocalCtxWatch(out, label);
}

// Member names every map view defines itself. A seeded key of one of these
// names cannot be an entry too, so it is refused rather than shadowed.
var __rhinoLocalCtxMapMethods = [
  "get", "size", "toString", "toJSON", "containsKey", "containsValue", "entrySet", "keySet", "values", "isEmpty",
  "put", "putAll", "remove", "clear", "forEach", "getOrDefault", "getClass", "hashCode", "equals",
  __rhinoLocalTagKey,
];

// A Java map as the context hands it over. Entries are enumerable accessors,
// so for...in and dot access see them; an entry whose value was not measured
// faults on read, and assigning one faults. get(k) answers like dot access,
// size() is measured, and String(v) only where `render` is given. `methods`
// overrides members of the faulting list, or `get`, with measured ones
// ({name: {arity, fn}}).
function __rhinoLocalCtxMap(label, keys, read, render, aliases, methods) {
  var map = {};
  var i;
  var define = function (key, enumerable, target) {
    // The caller's own measured methods are reserved too, so a seeded `keys`
    // cannot hide behind a context's keys() (Sol ctx r3 P2-3).
    if (__rhinoLocalCtxMapMethods.indexOf(key) !== -1 ||
      (methods && Object.prototype.hasOwnProperty.call(methods, key))) {
      throw __rhinoLocalCtxUnmeasured(label + "." + key,
        "as a seeded key (it shares its name with a Java Map method; the fixture cannot be represented)");
    }
    Object.defineProperty(map, key, {
      enumerable: enumerable,
      configurable: true,
      get: function () {
        return read(target);
      },
      set: function () {
        throw __rhinoLocalCtxUnmeasured(label + "." + key, "assignment");
      },
    });
  };
  for (i = 0; i < keys.length; i += 1) {
    define(keys[i], true, keys[i]);
  }
  var aliasKeys = aliases ? Object.keys(aliases) : [];
  for (i = 0; i < aliasKeys.length; i += 1) {
    if (keys.indexOf(aliasKeys[i]) === -1) {
      define(aliasKeys[i], false, aliases[aliasKeys[i]]);
    }
  }
  __rhinoLocalHide(map, "get", methods && methods.get ? null : function (key) {
    if (arguments.length !== 1) {
      throw __rhinoLocalCtxUnmeasured(label + ".get(" + __rhinoLocalArgShape(arguments) + ")");
    }
    var k = String(key);
    if (keys.indexOf(k) !== -1) {
      return read(k);
    }
    if (aliases && Object.prototype.hasOwnProperty.call(aliases, k)) {
      return read(aliases[k]);
    }
    throw __rhinoLocalCtxUnmeasured(label + ".get()", "of a key the map does not hold");
  });
  __rhinoLocalCtxMethod(map, label, "java.util.Map", "size", 0, function () {
    return keys.length;
  });
  __rhinoLocalHide(map, "toString", function () {
    if (render === null) {
      throw __rhinoLocalCtxUnmeasured("String(" + label + ")");
    }
    return render();
  });
  __rhinoLocalCtxFaulting(map, label, [
    "containsKey", "containsValue", "entrySet", "keySet", "values", "isEmpty", "put", "putAll", "remove", "clear",
    "forEach", "getOrDefault", "getClass", "hashCode", "equals",
  ].filter(function (name) {
    return !methods || !Object.prototype.hasOwnProperty.call(methods, name);
  }));
  if (methods) {
    Object.keys(methods).forEach(function (name) {
      __rhinoLocalCtxMethod(map, label, "java.util.Map", name, methods[name].arity, methods[name].fn);
    });
  }
  __rhinoLocalCtxToJSON(map, label);
  __rhinoLocalCtxTag(map, null, label);
  return __rhinoLocalCtxWatch(map, label);
}

// Header lookups are case-insensitive on AIC (measured: `["content-type"]` and
// `["Content-Type"]` both answer). A JS object cannot be, so each header is
// reachable by its seeded spelling, all lower case, and Title-Case.
function __rhinoLocalCtxHeaderAliases(names) {
  var aliases = {};
  var i;
  for (i = 0; i < names.length; i += 1) {
    Object.defineProperty(aliases, names[i].toLowerCase(), {
      value: names[i], enumerable: true, configurable: true, writable: true,
    });
    Object.defineProperty(aliases, names[i].toLowerCase().replace(/(^|-)([a-z])/g, function (m, dash, c) {
      return dash + c.toUpperCase();
    }), { value: names[i], enumerable: true, configurable: true, writable: true });
  }
  return aliases;
}

function __rhinoLocalCtxRenderMap(values, legacy) {
  var keys = Object.keys(values);
  var parts = [];
  var i;
  for (i = 0; i < keys.length; i += 1) {
    parts.push(legacy
      ? keys[i] + "=" + __rhinoLocalCtxRenderLegacy(values[keys[i]])
      : JSON.stringify(keys[i]) + ": " + __rhinoLocalCtxRenderNext(values[keys[i]]));
  }
  return legacy ? "{" + parts.join(", ") + "}" : "{ " + parts.join(", ") + " }";
}

// Seeded values only: AIC holds them as Java strings, lists and Integers.
function __rhinoLocalCtxRenderNext(value) {
  if (Array.isArray(value)) {
    var parts = [];
    var i;
    for (i = 0; i < value.length; i += 1) {
      parts.push(__rhinoLocalCtxRenderNext(value[i]));
    }
    return "[ " + parts.join(", ") + " ]";
  }
  if (typeof value === "string") {
    return JSON.stringify(value);
  }
  if (typeof value === "number" && value % 1 === 0) {
    return String(value);
  }
  throw __rhinoLocalCtxUnmeasured("String(map)", "holding a " + typeof value);
}

function __rhinoLocalCtxRenderLegacy(value) {
  if (Array.isArray(value)) {
    return "[" + value.map(__rhinoLocalCtxRenderLegacy).join(", ") + "]";
  }
  if (typeof value === "string" || (typeof value === "number" && value % 1 === 0)) {
    return String(value);
  }
  throw __rhinoLocalCtxUnmeasured("String(map)", "holding a " + typeof value);
}

// --- Script values handed to a setter ----------------------------------------
//
// True only for the shapes JSON describes exactly: a string, finite number,
// boolean, a dense ordinary array, or an object whose prototype is
// Object.prototype (or null) and whose own properties are all enumerable data
// properties, recursively. A Date, RegExp, getter, view or Java object is
// none of these, and runtime.cjs's __rhinoLocalIsPlainObject (any non-array
// object) is too loose to tell. `nested` refuses null inside a value, which
// no capture measured.
function __rhinoLocalCtxPlainJson(value) {
  var proto;
  var names;
  var i;
  var own;
  try {
    proto = Object.getPrototypeOf(value);
    names = Object.getOwnPropertyNames(value);
  } catch (e) {
    return false;
  }
  if (__rhinoLocalListRow(value) !== null) {
    return false;
  }
  if (Array.isArray(value)) {
    if (proto !== Array.prototype) {
      return false;
    }
    for (i = 0; i < names.length; i += 1) {
      if (names[i] === "length") {
        continue;
      }
      if (String(Number(names[i]) >>> 0) !== names[i] || Number(names[i]) >= value.length) {
        return false;
      }
      own = Object.getOwnPropertyDescriptor(value, names[i]);
      if (!("value" in own) || !own.enumerable) {
        return false;
      }
    }
    return names.length === value.length + 1;
  }
  if (proto !== Object.prototype && proto !== null) {
    return false;
  }
  for (i = 0; i < names.length; i += 1) {
    own = Object.getOwnPropertyDescriptor(value, names[i]);
    if (!("value" in own) || !own.enumerable) {
      return false;
    }
  }
  return true;
}

// `path` holds the objects on the active traversal path: meeting one again
// is a cycle, which faults rather than overflowing the stack into a catchable
// InternalError (Sol ctx r2 P2-1). The same object twice in an acyclic tree
// is not on the path the second time, and stays JSON.
function __rhinoLocalCtxJsonSafe(value, nested, path) {
  var i;
  var keys;
  var ok;
  if (value === null) {
    return !nested;
  }
  if (typeof value === "string" || typeof value === "boolean") {
    return true;
  }
  if (typeof value === "number") {
    return isFinite(value);
  }
  if (typeof value !== "object" || !__rhinoLocalCtxPlainJson(value)) {
    return false;
  }
  path = path || [];
  if (path.indexOf(value) !== -1) {
    return false;
  }
  path.push(value);
  ok = true;
  keys = Object.keys(value);
  for (i = 0; ok && i < keys.length; i += 1) {
    ok = __rhinoLocalCtxJsonSafe(value[keys[i]], true, path);
  }
  path.pop();
  return ok;
}

// A JSON object (not array) by the same rule.
function __rhinoLocalCtxIsJsonObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value) && __rhinoLocalCtxPlainJson(value);
}

// Copy a JSON-safe value into a fresh tree built with own data properties, so
// a `__proto__` key stays a key.
function __rhinoLocalCtxCopy(value) {
  var out;
  var keys;
  var i;
  if (value === null || typeof value !== "object") {
    return value;
  }
  if (Array.isArray(value)) {
    out = [];
    for (i = 0; i < value.length; i += 1) {
      out.push(__rhinoLocalCtxCopy(value[i]));
    }
    return out;
  }
  out = {};
  keys = Object.keys(value);
  for (i = 0; i < keys.length; i += 1) {
    Object.defineProperty(out, keys[i], {
      value: __rhinoLocalCtxCopy(value[keys[i]]), enumerable: true, configurable: true, writable: true,
    });
  }
  return out;
}

// --- Bindings several contexts share -------------------------------------------
//
// identity, scopes, requestProperties and clientProperties measured the same
// in the access-token and OIDC claims contexts (2026-11-10); where a member
// was measured in only one, the caller's `opts` says so.

// `opts.tag`: the measured Object.prototype.toString tag (`"JavaObject"` for
// may-act, 2026-11-10). Absent, the tag stays unmeasured and faults, as it
// did before that option existed. `opts.emptyAttribute`: the empty-list
// grant for next-gen getAttributeValues, citing the capture. Omitted, an
// empty attribute list faults (Sol ctx r11 P2-1).
function __rhinoLocalCtxBuildIdentity(seed, opts) {
  var legacy = __rhinoLocalCtx.engine === "legacy";
  // Next-gen: measured 2026-11-10, String(identity) is the wrapper's Java
  // toString (class@hash) while identity.toString() is "Cannot find function
  // toString", so the view has no toString at all and converts through
  // valueOf. A null prototype is what lets Rhino's own missing-member error
  // read as it does on AIC.
  var identity = __rhinoLocalCtx.engine === "legacy" ? {} : Object.create(null);
  var attrs = seed.attributes;
  if (legacy) {
    var cls = "com.sun.identity.idm.AMIdentity";
    // AMIdentity hands back the same String object on every call: measured
    // 2026-11-10 (access-token modification), `identity.getName() ===
    // identity.getName()` is true while getName() === getClientId() of the
    // same text is false. So each getter makes its string once.
    var names = {};
    var once = function (key, text) {
      if (!Object.prototype.hasOwnProperty.call(names, key)) {
        names[key] = __rhinoLocalCtxJavaString(text);
      }
      return names[key];
    };
    __rhinoLocalCtxMethod(identity, "identity", cls, "getName", 0, function () {
      return once("name", seed.name);
    });
    __rhinoLocalCtxMethod(identity, "identity", cls, "getUniversalId", 0, function () {
      return once("universalId", seed.universalId);
    });
    __rhinoLocalCtxMethod(identity, "identity", cls, "isExists", 0, function () {
      return seed.exists;
    });
    __rhinoLocalCtxMethod(identity, "identity", cls, "getAttribute", 1, function (name) {
      var key = String(name);
      // Measured 2026-09-10: an attribute the identity lacks is an empty set.
      var values = Object.prototype.hasOwnProperty.call(attrs, key) ? attrs[key] : [];
      return __rhinoLocalCtxJavaSet(values, "identity.getAttribute()", false);
    });
    // Measured: identity.exists() is "Cannot call property exists", Rhino's
    // refusal to call a non-function (a bean property of isExists()).
    __rhinoLocalHide(identity, "exists", seed.exists);
    __rhinoLocalHide(identity, "toString", function () {
      return seed.universalId;
    });
    // Never emulate getAttributes(): it returns the whole profile, password
    // included, in clear (.ai/core.md §8).
    __rhinoLocalCtxFaulting(identity, "identity", [
      "getAttributes", "setAttributes", "removeAttributes", "getRealm", "getType", "isActive", "store",
      "getMemberships", "isMember", "getAssignedServices", "getClass", "hashCode", "equals",
    ]);
  } else {
    var ngCls = "org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper";
    __rhinoLocalCtxMethod(identity, "identity", ngCls, "getName", 0, function () {
      return seed.name;
    });
    __rhinoLocalCtxMethod(identity, "identity", ngCls, "getUniversalId", 0, function () {
      return seed.universalId;
    });
    __rhinoLocalCtxMethod(identity, "identity", ngCls, "exists", 0, function () {
      return seed.exists;
    });
    __rhinoLocalCtxMethod(identity, "identity", ngCls, "getAttributeValues", 1, function (name) {
      var key = String(name);
      // Measured where the caller says so: an attribute the identity lacks
      // reads as an empty list. That empty surface is this binding's grant,
      // not the context kind's (Sol ctx r11 P2-1).
      return __rhinoLocalCtxList(Object.prototype.hasOwnProperty.call(attrs, key) ? attrs[key].slice() : [],
        undefined, undefined, opts && opts.emptyAttribute);
    });
    __rhinoLocalHide(identity, "valueOf", function () {
      return ngCls + "@rhino-local";
    });
    __rhinoLocalCtxFaulting(identity, "identity", ["setAttribute", "addAttribute", "store", "getClass", "hashCode",
      "equals", "wait", "notify", "notifyAll"]);
  }
  __rhinoLocalCtxToJSON(identity, "identity");
  // The tag is unmeasured except where the caller measured one (Sol ctx r2 P2-5).
  __rhinoLocalCtxTag(identity, opts && opts.tag ? opts.tag : null, "identity");
  return __rhinoLocalCtxWatch(identity, "identity");
}

// `opts.add`: next-gen add() works on this list (measured for the token
// context). `opts.toArray`: next-gen toArray() answers a JS array (measured
// for OIDC claims).
function __rhinoLocalCtxBuildScopes(items, opts) {
  if (__rhinoLocalCtx.engine === "legacy") {
    return __rhinoLocalCtxJavaSet(items, "scopes", false);
  }
  var store = items.slice();
  var list = __rhinoLocalCtxList(store, "scopes");
  // Measured on next-gen: contains() answers, and add() works on this list
  // (it is not the token's scope; setScope copies it).
  __rhinoLocalHide(list, "contains", function (v) {
    if (arguments.length !== 1) {
      throw __rhinoLocalCtxUnmeasured("scopes.contains(" + __rhinoLocalArgShape(arguments) + ")");
    }
    return store.indexOf(String(v)) !== -1;
  });
  __rhinoLocalHide(list, "add", function (v) {
    if (!opts || !opts.add) {
      throw __rhinoLocalCtxUnmeasured("scopes.add()");
    }
    if (arguments.length !== 1 || typeof v !== "string") {
      throw __rhinoLocalCtxUnmeasured("scopes.add(" + __rhinoLocalArgShape(arguments) + ")");
    }
    __rhinoLocalCtxCheck(list);
    store.push(v);
    list[store.length - 1] = v;
    __rhinoLocalHide(list, "length", store.length);
    list[__rhinoLocalListKey].items.push(v);
    __rhinoLocalCtxRewatch(list);
    return true;
  });
  if (opts && opts.toArray) {
    __rhinoLocalCtxMethod(list, "scopes", "java.util.List", "toArray", 0, function () {
      return store.slice();
    });
  }
  return list;
}

// `opts.paramsGetMissing`: requestParams.get() of an absent key is null
// (measured for OIDC claims); otherwise it faults. `opts.javaTags`: the value
// lists' tag is `[object JavaList]` (measured for validate scope).
// `opts.listKind`: the value lists' own kind, when it was not measured as
// the context's scope lists were (Sol ctx r5 P2-6, Sol ctx r10 P2-1).
// Omitted, they use the context kind. Access-token and OIDC pass a request
// kind that refuses a size-zero value: an empty identity attribute or
// claimsLocales list does not measure a request value.
function __rhinoLocalCtxBuildRequest(seed, realm, opts) {
  var legacy = __rhinoLocalCtx.engine === "legacy";
  var listOf = function (values) {
    var list = __rhinoLocalCtxList(values.map(__rhinoLocalCtxString), undefined, opts && opts.listKind);
    if (opts && opts.javaTags) {
      __rhinoLocalCtxTag(list, "JavaList", "a Java list");
    }
    return list;
  };
  var listMap = function (label, values, aliases, methods) {
    var lists = __rhinoLocalCtxBare();
    Object.keys(values).forEach(function (k) {
      lists[k] = listOf(values[k]);
    });
    return __rhinoLocalCtxMap(label, Object.keys(values), function (k) {
      return lists[k];
    }, function () {
      return __rhinoLocalCtxRenderMap(values, legacy);
    }, aliases, methods);
  };
  var params = listMap("requestProperties.requestParams", seed.params, null, opts && opts.paramsGetMissing
    ? {
      // Measured for OIDC claims (2026-11-10): get() of an absent parameter
      // is null, as Java's Map.get is.
      get: {
        arity: 1,
        fn: function (key) {
          var k = String(key);
          return Object.prototype.hasOwnProperty.call(seed.params, k) ? params[k] : null;
        },
      },
    }
    : null);
  var headers = listMap("requestProperties.requestHeaders", seed.headers,
    __rhinoLocalCtxHeaderAliases(Object.keys(seed.headers)));
  var values = {
    realm: __rhinoLocalCtxString(realm),
    requestHeaders: headers,
    requestParams: params,
    requestUri: __rhinoLocalCtxString(seed.uri),
  };
  return __rhinoLocalCtxMap("requestProperties", ["realm", "requestHeaders", "requestParams", "requestUri"],
    function (k) {
      return values[k];
    }, null);
}

// `opts.redirectUris`: next-gen redirectUris reads as a list (measured for
// OIDC claims); otherwise it is unmeasured. `opts.redirectKind`: that list's
// own kind, when it was measured apart from the context list (Sol ctx r10
// P2-1). Omitted, the context kind, which is what OIDC claims measured.
// `opts.redirectEmpty`: the empty-list grant for that list, citing the
// capture. Omitted, an empty redirect list faults (Sol ctx r11 P2-1).
// `opts.javaTags`: customProperties is `[object JavaMap]` and redirectUris
// `[object JavaList]` (measured for validate scope). `opts.setTag`: the legacy
// grant/scope/response sets' tag
// (may-act measured `[object JavaObject]`); the sets are built on read, after
// the seal, so the tag has to be applied before their own watch.
function __rhinoLocalCtxBuildClient(seed, opts) {
  var legacy = __rhinoLocalCtx.engine === "legacy";
  var custom = __rhinoLocalCtxMap("clientProperties.customProperties", Object.keys(seed.customProperties),
    function () {
      throw __rhinoLocalCtxUnmeasured("clientProperties.customProperties", "entry values");
    }, Object.keys(seed.customProperties).length === 0 ? function () {
      return legacy ? "{}" : "{  }";
    } : null);
  if (opts && opts.javaTags) {
    __rhinoLocalCtxTag(custom, "JavaMap", "clientProperties.customProperties");
  }
  var sets = {
    allowedGrantTypes: { values: seed.allowedGrantTypes, javaClass: "java.util.HashSet" },
    allowedScopes: { values: seed.allowedScopes, javaClass: "java.util.TreeSet" },
    allowedResponseTypes: { values: seed.allowedResponseTypes, javaClass: "java.util.LinkedHashSet" },
  };
  var keys = legacy
    ? ["allowedGrantTypes", "clientId", "customProperties", "allowedScopes", "allowedResponseTypes"]
    : ["allowedGrantTypes", "clientId", "customProperties", "redirectUris", "allowedScopes", "allowedResponseTypes"];
  return __rhinoLocalCtxMap("clientProperties", keys, function (k) {
    if (k === "clientId") {
      return __rhinoLocalCtxString(seed.clientId);
    }
    if (k === "customProperties") {
      return custom;
    }
    if (k === "redirectUris") {
      if (!opts || !opts.redirectUris) {
        throw __rhinoLocalCtxUnmeasured("clientProperties.redirectUris");
      }
      // Built here rather than through __rhinoLocalCtxList, because a read
      // happens after the seal and a tag added to a watched view would read
      // as the script's own mutation.
      var redirectKind = opts && opts.redirectKind ? opts.redirectKind : __rhinoLocalCtx.listKind;
      var uris = __rhinoLocalListView(seed.redirectUris.slice(), redirectKind, opts && opts.redirectEmpty);
      __rhinoLocalCtxTag(uris, opts.javaTags ? "JavaList" : null, "clientProperties.redirectUris");
      return __rhinoLocalCtxWatch(uris, "clientProperties.redirectUris");
    }
    if (legacy) {
      return __rhinoLocalCtxJavaSet(sets[k].values, "clientProperties." + k, false, undefined, opts && opts.setTag);
    }
    // Measured: next-gen hands over the raw Java set, which the context's
    // class shutter refuses.
    throw __rhinoLocalShutterError(sets[k].javaClass);
  }, null);
}


// Reset per run, from a context seed.
function __rhinoLocalCtxBegin(engine) {
  __rhinoLocalCtx.engine = engine || "next-gen";
  __rhinoLocalCtx.views = [];
  __rhinoLocalCtx.sealed = false;
  __rhinoLocalCtx.realJavaStrings = false;
}

function __rhinoLocalCtxUnbind(names) {
  var i;
  for (i = 0; i < names.length; i += 1) {
    delete __rhinoLocalTop[names[i]];
  }
}
