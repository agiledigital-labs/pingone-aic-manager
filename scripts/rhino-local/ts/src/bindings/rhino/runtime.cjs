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
  managed: {},
  esv: {},
  httpStubs: [],
  generatedId: 0,
  outcome: null,
  callbacks: [],
  openidm: [],
  http: [],
  logs: [],
};

var sharedState;
var transientState;
var systemEnv;

function __rhinoLocalClone(value) {
  return JSON.parse(JSON.stringify(value));
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

function __rhinoLocalExpectArity(path, args, expected) {
  if (args.length !== expected) {
    throw new Error(
      "rhino-local: " +
        path +
        " arity=" +
        args.length +
        " (expected " +
        expected +
        ")"
    );
  }
}

function __rhinoLocalHas(object, key) {
  return Object.prototype.hasOwnProperty.call(object, key);
}

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
  return list;
}

function __rhinoLocalRequestMap(values, options) {
  var caseInsensitive = Boolean(options && options.caseInsensitive);
  var asList = !(options && options.asList === false);
  var map = {};
  var store = {};
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
        store[stored] = __rhinoLocalJavaList([value]);
      } else {
        store[stored] = __rhinoLocalJavaList(value);
      }
    } else {
      store[stored] = value;
    }
    map[stored] = store[stored];
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
    throw new Error(
      "rhino-local: keySet is blocked by AM's class shutter; iterate with for...in"
    );
  });
  return map;
}

function __rhinoLocalStateMap(bucketName) {
  var map = {};
  __rhinoLocalHide(map, "get", function (key) {
    __rhinoLocalExpectArity(bucketName + ".get", arguments, 1);
    var k = String(key);
    if (!__rhinoLocalHas(__rhinoLocal[bucketName], k)) {
      return null;
    }
    return __rhinoLocal[bucketName][k];
  });
  __rhinoLocalHide(map, "put", function (key, value) {
    __rhinoLocalExpectArity(bucketName + ".put", arguments, 2);
    if (value === undefined) {
      throw new Error("rhino-local: " + bucketName + ".put: value is undefined");
    }
    __rhinoLocal[bucketName][String(key)] = value;
  });
  return map;
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
  return __rhinoLocalLookupState(String(key));
};

nodeState.isDefined = function (key) {
  __rhinoLocalExpectArity("nodeState.isDefined", arguments, 1);
  return __rhinoLocalIsDefined(String(key));
};

nodeState.putShared = function (key, value) {
  __rhinoLocalExpectArity("nodeState.putShared", arguments, 2);
  if (value === undefined) {
    throw new Error("rhino-local: nodeState.putShared: value is undefined");
  }
  __rhinoLocal.shared[String(key)] = value;
  return nodeState;
};

nodeState.putTransient = function (key, value) {
  __rhinoLocalExpectArity("nodeState.putTransient", arguments, 2);
  if (value === undefined) {
    throw new Error("rhino-local: nodeState.putTransient: value is undefined");
  }
  __rhinoLocal.transient[String(key)] = value;
  return nodeState;
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
        out += String(bound[argIndex]);
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

function __rhinoLocalLog(level, args) {
  if (args.length < 1) {
    throw new Error(
      "rhino-local: logger." + level + " arity=" + args.length + " (expected >= 1)"
    );
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
  return scriptName;
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
  return true;
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
    throw new Error(
      "rhino-local: callbacksBuilder.nameCallback arity=" +
        arguments.length +
        " (expected 1 or 2)"
    );
  }
  var fields = { prompt: String(prompt) };
  if (arguments.length > 1) {
    fields.defaultName = String(defaultName);
  }
  __rhinoLocalCallback("NameCallback", fields);
};

callbacksBuilder.passwordCallback = function (prompt, echoOn) {
  __rhinoLocalExpectArity("callbacksBuilder.passwordCallback", arguments, 2);
  __rhinoLocalCallback("PasswordCallback", {
    prompt: String(prompt),
    echoOn: Boolean(echoOn),
  });
};

callbacksBuilder.textOutputCallback = function (messageType, message) {
  __rhinoLocalExpectArity("callbacksBuilder.textOutputCallback", arguments, 2);
  __rhinoLocalCallback("TextOutputCallback", {
    messageType: messageType,
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
    fields.messageType = a[0];
    fields.options = a[1];
    fields.defaultOption = a[2];
  } else if (a.length === 3 && typeof a[0] === "number") {
    fields.messageType = a[0];
    fields.optionType = a[1];
    fields.defaultOption = a[2];
  } else if (a.length === 4 && Array.isArray(a[2])) {
    fields.prompt = String(a[0]);
    fields.messageType = a[1];
    fields.options = a[2];
    fields.defaultOption = a[3];
  } else if (a.length === 4) {
    fields.prompt = String(a[0]);
    fields.messageType = a[1];
    fields.optionType = a[2];
    fields.defaultOption = a[3];
  } else {
    throw new Error(
      "rhino-local: callbacksBuilder.confirmationCallback arity=" + a.length
    );
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
    multipleSelectionsAllowed: Boolean(multipleSelectionsAllowed),
  });
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

function __rhinoLocalRequireCollection(method, collection) {
  if (!__rhinoLocalHas(__rhinoLocal.managed, collection)) {
    throw new Error(
      "rhino-local: openidm." +
        method +
        ": no given.managed entry for " +
        JSON.stringify(collection)
    );
  }
  return __rhinoLocal.managed[collection];
}

function __rhinoLocalRequireRecord(method, resource) {
  var split = __rhinoLocalSplitResource(resource);
  if (split.recordId === null) {
    throw new Error(
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
    throw new Error(
      "rhino-local: openidm." +
        method +
        ": no given.managed entry for " +
        JSON.stringify(resource)
    );
  }
  return found;
}

function __rhinoLocalProject(record, fields) {
  if (!fields || !Array.isArray(fields) || fields.length === 0) {
    return record;
  }
  var i;
  for (i = 0; i < fields.length; i += 1) {
    if (fields[i] === "*") {
      return record;
    }
  }
  var out = {};
  if (record._id !== undefined) {
    out._id = record._id;
  }
  if (record._rev !== undefined) {
    out._rev = record._rev;
  }
  var field;
  var parent;
  for (i = 0; i < fields.length; i += 1) {
    field = String(fields[i]);
    if (__rhinoLocalHas(record, field)) {
      out[field] = record[field];
    } else if (field.indexOf("/") !== -1) {
      parent = field.split("/")[0];
      if (__rhinoLocalHas(record, parent) && !__rhinoLocalHas(out, parent)) {
        out[parent] = record[parent];
      }
    }
  }
  return out;
}

function __rhinoLocalPushOpenidm(method, resource, body, actionName) {
  var rec = { method: method, resource: resource };
  if (body !== undefined) {
    rec.body = __rhinoLocalClone(body);
  }
  if (actionName !== undefined) {
    rec.actionName = actionName;
  }
  __rhinoLocal.openidm.push(rec);
}

function __rhinoLocalFilterError(filter) {
  throw new Error(
    "rhino-local: openidm.query: unmocked filter " + JSON.stringify(filter)
  );
}

function __rhinoLocalIsIdentChar(ch) {
  return (
    (ch >= "a" && ch <= "z") ||
    (ch >= "A" && ch <= "Z") ||
    (ch >= "0" && ch <= "9") ||
    ch === "_" ||
    ch === "-"
  );
}

function __rhinoLocalReadField(record, pointer) {
  var path = String(pointer);
  if (path.charAt(0) === "/") {
    path = path.substring(1);
  }
  if (path === "") {
    return record;
  }
  var parts = path.split("/");
  var cur = record;
  var i;
  for (i = 0; i < parts.length; i += 1) {
    if (cur === null || cur === undefined || typeof cur !== "object") {
      return undefined;
    }
    if (!__rhinoLocalHas(cur, parts[i])) {
      return undefined;
    }
    cur = cur[parts[i]];
  }
  return cur;
}

function __rhinoLocalCompareOne(actual, op, expected) {
  var left = actual === null || actual === undefined ? "" : String(actual);
  var right = expected === null || expected === undefined ? "" : String(expected);
  if (op === "eq") {
    return left === right;
  }
  if (op === "co") {
    return left.indexOf(right) !== -1;
  }
  if (op === "sw") {
    return left.indexOf(right) === 0;
  }
  return false;
}

function __rhinoLocalCompareField(record, pointer, op, expected) {
  var actual = __rhinoLocalReadField(record, pointer);
  if (op === "pr") {
    return actual !== undefined && actual !== null;
  }
  if (actual === undefined || actual === null) {
    return false;
  }
  if (Array.isArray(actual)) {
    var i;
    for (i = 0; i < actual.length; i += 1) {
      if (__rhinoLocalCompareOne(actual[i], op, expected)) {
        return true;
      }
    }
    return false;
  }
  return __rhinoLocalCompareOne(actual, op, expected);
}

function __rhinoLocalParseFilter(filter) {
  var src = String(filter);
  var i = 0;

  function skipWs() {
    while (i < src.length) {
      var ch = src.charAt(i);
      if (ch !== " " && ch !== "\t" && ch !== "\n" && ch !== "\r") {
        break;
      }
      i += 1;
    }
  }

  function peek() {
    skipWs();
    if (i >= src.length) {
      return "";
    }
    return src.charAt(i);
  }

  function readIdent() {
    skipWs();
    var start = i;
    while (i < src.length && __rhinoLocalIsIdentChar(src.charAt(i))) {
      i += 1;
    }
    if (start === i) {
      return "";
    }
    return src.substring(start, i);
  }

  function readField() {
    skipWs();
    var start = i;
    if (i < src.length && src.charAt(i) === "/") {
      i += 1;
    }
    if (!__rhinoLocalIsIdentChar(peekCharRaw())) {
      i = start;
      return null;
    }
    while (i < src.length) {
      var ch = src.charAt(i);
      if (__rhinoLocalIsIdentChar(ch) || ch === "/") {
        i += 1;
      } else {
        break;
      }
    }
    var field = src.substring(start, i);
    if (field === "" || field === "/") {
      i = start;
      return null;
    }
    return field;
  }

  function peekCharRaw() {
    if (i >= src.length) {
      return "";
    }
    return src.charAt(i);
  }

  function readString() {
    skipWs();
    if (src.charAt(i) !== '"') {
      return null;
    }
    i += 1;
    var out = "";
    while (i < src.length) {
      var ch = src.charAt(i);
      if (ch === "\\") {
        i += 1;
        if (i >= src.length) {
          __rhinoLocalFilterError(filter);
        }
        out += src.charAt(i);
        i += 1;
      } else if (ch === '"') {
        i += 1;
        return out;
      } else {
        out += ch;
        i += 1;
      }
    }
    __rhinoLocalFilterError(filter);
    return null;
  }

  function readNumber() {
    skipWs();
    var start = i;
    if (src.charAt(i) === "-") {
      i += 1;
    }
    var digits = 0;
    while (i < src.length && src.charAt(i) >= "0" && src.charAt(i) <= "9") {
      digits += 1;
      i += 1;
    }
    if (src.charAt(i) === ".") {
      i += 1;
      while (i < src.length && src.charAt(i) >= "0" && src.charAt(i) <= "9") {
        digits += 1;
        i += 1;
      }
    }
    if (digits === 0) {
      i = start;
      return null;
    }
    return Number(src.substring(start, i));
  }

  function parseValue() {
    skipWs();
    var str = readString();
    if (str !== null) {
      return str;
    }
    var ident = "";
    var saved = i;
    ident = readIdent();
    if (ident === "true") {
      return true;
    }
    if (ident === "false") {
      return false;
    }
    if (ident === "null") {
      return null;
    }
    i = saved;
    var num = readNumber();
    if (num !== null && !isNaN(num)) {
      return num;
    }
    __rhinoLocalFilterError(filter);
    return null;
  }

  function parsePrimary() {
    skipWs();
    if (peek() === "(") {
      i += 1;
      var inner = parseOr();
      skipWs();
      if (peek() !== ")") {
        __rhinoLocalFilterError(filter);
      }
      i += 1;
      return inner;
    }
    var saved = i;
    var ident = readIdent();
    if (ident === "true") {
      return { kind: "true" };
    }
    if (ident === "false") {
      return { kind: "false" };
    }
    i = saved;
    var field = readField();
    if (field === null) {
      __rhinoLocalFilterError(filter);
    }
    skipWs();
    var op = readIdent();
    if (op === "pr") {
      return { kind: "pr", field: field };
    }
    if (op === "eq" || op === "co" || op === "sw") {
      return { kind: "cmp", field: field, op: op, value: parseValue() };
    }
    __rhinoLocalFilterError(filter);
    return null;
  }

  function parseNot() {
    skipWs();
    if (peek() === "!") {
      i += 1;
      return { kind: "not", inner: parseNot() };
    }
    var saved = i;
    var ident = readIdent();
    if (ident === "not") {
      // AIC rejects the word form (docs/api/10, verified 2026-07-03).
      __rhinoLocalFilterError(filter);
    }
    i = saved;
    return parsePrimary();
  }

  function parseAnd() {
    var left = parseNot();
    while (true) {
      var saved = i;
      var ident = readIdent();
      if (ident !== "and") {
        i = saved;
        break;
      }
      left = { kind: "and", left: left, right: parseNot() };
    }
    return left;
  }

  function parseOr() {
    var left = parseAnd();
    while (true) {
      var saved = i;
      var ident = readIdent();
      if (ident !== "or") {
        i = saved;
        break;
      }
      left = { kind: "or", left: left, right: parseAnd() };
    }
    return left;
  }

  var ast = parseOr();
  skipWs();
  if (i !== src.length) {
    __rhinoLocalFilterError(filter);
  }
  return ast;
}

function __rhinoLocalEvalFilter(record, ast) {
  if (ast.kind === "true") {
    return true;
  }
  if (ast.kind === "false") {
    return false;
  }
  if (ast.kind === "not") {
    return !__rhinoLocalEvalFilter(record, ast.inner);
  }
  if (ast.kind === "and") {
    return (
      __rhinoLocalEvalFilter(record, ast.left) &&
      __rhinoLocalEvalFilter(record, ast.right)
    );
  }
  if (ast.kind === "or") {
    return (
      __rhinoLocalEvalFilter(record, ast.left) ||
      __rhinoLocalEvalFilter(record, ast.right)
    );
  }
  if (ast.kind === "pr") {
    return __rhinoLocalCompareField(record, ast.field, "pr", null);
  }
  if (ast.kind === "cmp") {
    return __rhinoLocalCompareField(record, ast.field, ast.op, ast.value);
  }
  return false;
}

function __rhinoLocalMatchesFilter(record, filter) {
  if (!filter || filter === "true") {
    return true;
  }
  if (filter === "false") {
    return false;
  }
  return __rhinoLocalEvalFilter(record, __rhinoLocalParseFilter(filter));
}

openidm.read = function (resourceName, _params, fields) {
  if (arguments.length < 1 || arguments.length > 3) {
    throw new Error(
      "rhino-local: openidm.read arity=" + arguments.length + " (expected 1..3)"
    );
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("read", resource);
  var found = __rhinoLocalRequireRecord("read", resource);
  return __rhinoLocalProject(found.record, fields);
};

openidm.query = function (resourceName, params, fields) {
  if (arguments.length < 2 || arguments.length > 3) {
    throw new Error(
      "rhino-local: openidm.query arity=" + arguments.length + " (expected 2 or 3)"
    );
  }
  var collection = String(resourceName);
  __rhinoLocalPushOpenidm("query", collection, params);
  var rows = __rhinoLocalRequireCollection("query", collection);
  var filter = params && params._queryFilter;
  var result = [];
  var i;
  for (i = 0; i < rows.length; i += 1) {
    if (__rhinoLocalMatchesFilter(rows[i], filter)) {
      result.push(__rhinoLocalProject(rows[i], fields));
    }
  }
  return {
    result: result,
    resultCount: result.length,
    pagedResultsCookie: null,
    totalPagedResultsPolicy: "NONE",
    totalPagedResults: -1,
  };
};

openidm.create = function (resourceName, newResourceId, content) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw new Error(
      "rhino-local: openidm.create arity=" + arguments.length + " (expected 3..5)"
    );
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
  if (!__rhinoLocalHas(__rhinoLocal.managed, collection)) {
    __rhinoLocal.managed[collection] = [];
  }
  var stored = __rhinoLocalClone(content);
  stored._id = id;
  if (stored._rev === undefined) {
    stored._rev = "0";
  }
  __rhinoLocal.managed[collection].push(stored);
  return stored;
};

openidm.update = function (id, _rev, value) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw new Error(
      "rhino-local: openidm.update arity=" + arguments.length + " (expected 3..5)"
    );
  }
  var resource = String(id);
  __rhinoLocalPushOpenidm("update", resource, value);
  var found = __rhinoLocalRequireRecord("update", resource);
  var next = __rhinoLocalClone(value);
  next._id = found.record._id;
  if (next._rev === undefined) {
    next._rev = found.record._rev;
  }
  found.rows[found.index] = next;
  return next;
};

openidm.patch = function (resourceName, _rev, patch) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw new Error(
      "rhino-local: openidm.patch arity=" + arguments.length + " (expected 3..5)"
    );
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("patch", resource, patch);
  var found = __rhinoLocalRequireRecord("patch", resource);
  var i;
  var op;
  var field;
  for (i = 0; i < patch.length; i += 1) {
    op = patch[i];
    field = String(op.field).replace(/^\//, "");
    if (op.operation === "remove") {
      delete found.record[field];
    } else {
      found.record[field] = op.value;
    }
  }
  return found.record;
};

openidm.delete = function (resourceName) {
  if (arguments.length < 2 || arguments.length > 4) {
    throw new Error(
      "rhino-local: openidm.delete arity=" + arguments.length + " (expected 2..4)"
    );
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("delete", resource);
  var found = __rhinoLocalRequireRecord("delete", resource);
  var removed = found.record;
  found.rows.splice(found.index, 1);
  return removed;
};

openidm.action = function (resource, actionName, content, params) {
  if (arguments.length < 2 || arguments.length > 5) {
    throw new Error(
      "rhino-local: openidm.action arity=" + arguments.length + " (expected 2..5)"
    );
  }
  var body = content;
  if (body === undefined || body === null) {
    body = params;
  }
  __rhinoLocalPushOpenidm("action", String(resource), body, String(actionName));
  return {};
};

function __rhinoLocalAsPattern(value) {
  if (value && typeof value === "object" && typeof value.__regex === "string") {
    return new RegExp(value.__regex, value.__flags || "");
  }
  return value;
}

function __rhinoLocalMatchUrl(pattern, url) {
  var p = __rhinoLocalAsPattern(pattern);
  if (typeof p === "string") {
    return p === url;
  }
  if (p && typeof p.test === "function") {
    return p.test(url);
  }
  throw new Error("rhino-local: given.http match.url is not a string or regexp");
}

function __rhinoLocalMatchHttp(url, method) {
  var i;
  var stub;
  var wantMethod;
  for (i = 0; i < __rhinoLocal.httpStubs.length; i += 1) {
    stub = __rhinoLocal.httpStubs[i];
    if (!__rhinoLocalMatchUrl(stub.match.url, url)) {
      continue;
    }
    wantMethod = stub.match.method;
    if (wantMethod && String(wantMethod).toUpperCase() !== method) {
      continue;
    }
    return stub.reply;
  }
  return null;
}

httpClient.send = function (uri, requestOptions) {
  if (arguments.length < 1 || arguments.length > 2) {
    throw new Error(
      "rhino-local: httpClient.send arity=" + arguments.length + " (expected 1 or 2)"
    );
  }
  var url = String(uri);
  var method = "GET";
  var body;
  if (requestOptions && requestOptions.method) {
    method = String(requestOptions.method).toUpperCase();
  }
  if (requestOptions && requestOptions.body !== undefined) {
    body = requestOptions.body;
  }
  var effect = { url: url, method: method };
  if (body !== undefined) {
    effect.body = body;
  }
  __rhinoLocal.http.push(effect);
  var reply = __rhinoLocalMatchHttp(url, method);
  if (!reply) {
    throw new Error(
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

idRepository.getIdentity = function (userName) {
  __rhinoLocalExpectArity("idRepository.getIdentity", arguments, 1);
  var wanted = String(userName);
  var collections = Object.keys(__rhinoLocal.managed);
  var c;
  var rows;
  var i;
  var record;
  var found = null;
  for (c = 0; c < collections.length; c += 1) {
    rows = __rhinoLocal.managed[collections[c]];
    for (i = 0; i < rows.length; i += 1) {
      record = rows[i];
      if (String(record._id) === wanted || String(record.userName) === wanted) {
        found = record;
        break;
      }
    }
    if (found) {
      break;
    }
  }
  if (!found) {
    throw new Error(
      "rhino-local: idRepository.getIdentity: no given.managed record for " +
        JSON.stringify(wanted)
    );
  }
  return {
    getName: function () {
      if (found.userName !== undefined) {
        return found.userName;
      }
      return found._id;
    },
    getUniversalId: function () {
      return found._id;
    },
    exists: function () {
      return true;
    },
    getAttributeValues: function (attributeName) {
      var value = found[attributeName];
      if (value === undefined || value === null) {
        return __rhinoLocalJavaList([]);
      }
      if (Array.isArray(value)) {
        return __rhinoLocalJavaList(value);
      }
      return __rhinoLocalJavaList([value]);
    },
    setAttribute: function () {
      __rhinoLocalNotMocked("idRepository.getIdentity()", "setAttribute", arguments, [
        {
          arity: 2,
          types: ["string", "array"],
          label: "setAttribute(attributeName: string, attributeValues: array)",
        },
      ]);
    },
    addAttribute: function () {
      __rhinoLocalNotMocked("idRepository.getIdentity()", "addAttribute", arguments, [
        {
          arity: 2,
          types: ["string", "string"],
          label: "addAttribute(attributeName: string, attributeValue: string)",
        },
      ]);
    },
    store: function () {
      __rhinoLocalNotMocked("idRepository.getIdentity()", "store", arguments, [
        { arity: 0, types: [], label: "store()" },
      ]);
    },
  };
};

systemEnv = {
  getProperty: function (key) {
    __rhinoLocalExpectArity("systemEnv.getProperty", arguments, 1);
    var k = String(key);
    if (!__rhinoLocalHas(__rhinoLocal.esv, k)) {
      throw new Error(
        "rhino-local: systemEnv.getProperty: no given.esv entry for " +
          JSON.stringify(k)
      );
    }
    return __rhinoLocal.esv[k];
  },
};

function __rhinoLocalSeed(given) {
  given = given || {};
  if (given.bindings) {
    throw new Error(
      "rhino-local: given.bindings seeding is not implemented; use a dedicated given.* field"
    );
  }
  __rhinoLocal.shared = __rhinoLocalClone(given.sharedState || {});
  __rhinoLocal.transient = __rhinoLocalClone(given.transientState || {});
  __rhinoLocal.secure = __rhinoLocalClone(given.secureState || {});
  __rhinoLocal.initialShared = __rhinoLocalClone(__rhinoLocal.shared);
  __rhinoLocal.initialTransient = __rhinoLocalClone(__rhinoLocal.transient);
  __rhinoLocal.initialSecure = __rhinoLocalClone(__rhinoLocal.secure);
  __rhinoLocal.managed = __rhinoLocalClone(given.managed || {});
  __rhinoLocal.esv = __rhinoLocalClone(given.esv || {});
  __rhinoLocal.httpStubs = given.http ? __rhinoLocalClone(given.http) : [];
  __rhinoLocal.generatedId = 0;
  __rhinoLocal.outcome = null;
  __rhinoLocal.callbacks = [];
  __rhinoLocal.openidm = [];
  __rhinoLocal.http = [];
  __rhinoLocal.logs = [];

  requestHeaders = __rhinoLocalRequestMap(given.requestHeaders || {}, {
    caseInsensitive: true,
    asList: true,
  });
  requestParameters = __rhinoLocalRequestMap(given.requestParameters || {}, {
    caseInsensitive: false,
    asList: true,
  });
  requestCookies = __rhinoLocalRequestMap(given.requestCookies || {}, {
    caseInsensitive: false,
    asList: false,
  });

  if (given.realm !== undefined) {
    realm = given.realm;
  }
  if (given.scriptName !== undefined) {
    scriptName = given.scriptName;
  }
  if (given.cookieName !== undefined) {
    cookieName = given.cookieName;
  }
  if (given.resumedFromSuspend !== undefined) {
    resumedFromSuspend = given.resumedFromSuspend;
  }
  if (given.locales !== undefined) {
    locales = given.locales;
  }

  if (given.engine === "legacy") {
    sharedState = __rhinoLocalStateMap("shared");
    transientState = __rhinoLocalStateMap("transient");
  }
}

function __rhinoLocalHarvest() {
  var recordedOutcome = __rhinoLocal.outcome;
  if (
    recordedOutcome === null &&
    typeof outcome !== "undefined" &&
    outcome !== null
  ) {
    recordedOutcome = String(outcome);
  }
  return JSON.stringify({
    outcome: recordedOutcome,
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
    callbacks: __rhinoLocal.callbacks,
    openidm: __rhinoLocal.openidm,
    http: __rhinoLocal.http,
    logs: __rhinoLocal.logs,
  });
}
