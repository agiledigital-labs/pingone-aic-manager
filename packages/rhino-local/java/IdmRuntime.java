import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.EcmaError;
import org.mozilla.javascript.Function;
import org.mozilla.javascript.JavaScriptException;
import org.mozilla.javascript.NativeJavaObject;
import org.mozilla.javascript.RhinoException;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;
import org.mozilla.javascript.WrappedException;
import org.mozilla.javascript.tools.shell.Global;

/** IDM execution and Java binding surfaces; storage and CREST routing belong to the host. */
final class IdmRuntime {
  private final Global scope;
  private final IdmValues values;
  private final HostCalls calls;
  private final List<Object> logs = java.util.Collections.synchronizedList(new ArrayList<Object>());
  private final java.util.Set<String> missing = new java.util.concurrent.ConcurrentSkipListSet<String>();
  private final Map<String, Object> bindings;
  private final Map<String, Object> properties;
  private final java.util.Set<String> assumed = new java.util.concurrent.ConcurrentSkipListSet<String>();
  /** E15: require() asks the host for IDM runtime-script sources instead of refusing. */
  private final boolean hostRequire;
  /** Module objects by IDM path, for this invocation only (IDM's module cache scope). */
  private final Map<String, Scriptable> modules = new java.util.concurrent.ConcurrentHashMap<String, Scriptable>();

  private IdmRuntime(Context cx, HostCalls calls, Map<String, Object> job, IdmContextFactory factory) {
    scope = IdmScope.create(cx, factory);
    values = new IdmValues(scope);
    calls.values = values;
    this.calls = calls;
    properties = job.get("properties") == null ? new LinkedHashMap<String, Object>() : Json.asObject(job.get("properties"), "properties");
    bindings = job.get("bindings") == null ? new LinkedHashMap<String, Object>() : Json.asObject(job.get("bindings"), "bindings");
    if (job.get("bindingAliases") != null) {
      for (Map.Entry<String, Object> alias : Json.asObject(job.get("bindingAliases"), "bindingAliases").entrySet()) {
        String target = Json.asString(alias.getValue(), "binding alias");
        if (!bindings.containsKey(target)) throw new IllegalArgumentException("unknown binding alias target: " + target);
        bindings.put(alias.getKey(), bindings.get(target));
      }
    }
    hostRequire = Boolean.TRUE.equals(job.get("idmRequire"));
    List<String> resourceNames = Json.optionalStringList(job, "resourceNameBindings");
    boolean crestBindings = Boolean.TRUE.equals(job.get("crestBindings"));
    for (Map.Entry<String, Object> e : bindings.entrySet()) {
      if (crestBindings && "context".equals(e.getKey())) IdmValues.prepareHttpHeaders(e.getValue());
      Object value = resourceNames != null && resourceNames.contains(e.getKey())
          ? new NativeJavaObject(scope, new ResourceName(Json.asString(e.getValue(), e.getKey())), ResourceName.class)
          : crestBindings && "request".equals(e.getKey()) ? values.wrapRequest(e.getValue()) : values.wrap(e.getValue());
      ScriptableObject.putProperty(scope.getPrototype(), e.getKey(), value);
    }
    install();
  }

  static Map<String, Object> eval(String id, Map<String, Object> job) {
    String source = Json.optionalString(job, "source");
    if (source == null) return Runner.errorResponse(id, "protocol_error", new IllegalArgumentException("job is missing source"), null, -1, -1, null);
    String sourceName = Json.optionalString(job, "sourceName");
    if (sourceName == null || sourceName.isEmpty()) sourceName = id;
    IdmContextFactory factory = new IdmContextFactory(Json.optionalLong(job, "timeoutMs", Runner.DEFAULT_TIMEOUT_MS));
    Context cx = factory.enterIdmContext();
    HostCalls calls = null;
    IdmRuntime runtime = null;
    Map<String, Object> response;
    boolean compiling = false;
    Object internalResult = null;
    try {
      calls = new HostCalls(id, factory);
      runtime = new IdmRuntime(cx, calls, job, factory);
      String preamble = Json.optionalString(job, "preamble");
      if (preamble != null) {
        compiling = true;
        org.mozilla.javascript.Script preambleScript = cx.compileString(preamble,
          job.get("preambleName") == null ? "<preamble>" : Json.asString(job.get("preambleName"), "preambleName"), 1, null);
        compiling = false;
        preambleScript.exec(cx, runtime.scope);
      }
      compiling = true;
      org.mozilla.javascript.Script script = cx.compileString(source, sourceName, 1, null);
      compiling = false;
      Object result = script.exec(cx, runtime.scope);
      String resultGlobal = Json.optionalString(job, "resultGlobal");
      if (resultGlobal != null) result = ScriptableObject.getProperty(runtime.scope, resultGlobal);
      if (Boolean.TRUE.equals(job.get("internalValues"))) internalResult = IdmValues.internalValue(result);
      response = Runner.response(id, "ok", JsValues.of(null), null);
      response.put("valueKind", result == Scriptable.NOT_FOUND ? "missing" : Undefined.isUndefined(result) ? "undefined"
          : result == null ? "null" : result instanceof Scriptable ? "object" : result instanceof CharSequence ? "string"
          : result instanceof Number ? "number" : result instanceof Boolean ? "boolean" : "object");
      response.put("value", IdmValues.javaValue(result));
    } catch (RhinoException e) {
      response = Runner.errorFromRhino(id, compiling ? "compile_error" : "runtime_error", e);
      response.put("thrown", thrown(e));
    } catch (Throwable e) {
      if (runtime == null && calls != null) calls.fail(new EmulationFailure(
          e instanceof IllegalArgumentException ? "protocol" : "internal", "IDM protocol setup: " + e.getMessage()));
      response = e instanceof Error && Runner.isInterrupt((Error) e) ? Runner.timeoutResponse(id)
          : Runner.errorResponse(id, "runtime_error", e, sourceName, -1, -1, null);
      Map<String, Object> thrown = new LinkedHashMap<String, Object>();
      thrown.put("kind", "java");
      thrown.put("class", e.getClass().getName());
      thrown.put("message", e.getMessage());
      response.put("thrown", thrown);
    } finally {
      factory.finish();
      if (calls != null) calls.close();
      Context.exit();
    }
    if (factory.failure != null) {
      response = Runner.errorResponse(id, "timeout".equals(factory.failure.info.get("kind")) ? "timeout" : "runtime_error", factory.failure, sourceName, -1, -1, null);
      Map<String, Object> fatal = new LinkedHashMap<String, Object>(factory.failure.info);
      if (!factory.missing.isEmpty()) fatal.put("missing", new ArrayList<String>(factory.missing));
      response.put("emulation", fatal);
    } else if ("timeout".equals(response.get("outcome"))) {
      response.put("emulation", new EmulationFailure("timeout", "IDM harness timeout").info);
    }
    if (calls != null && !calls.unanswered.isEmpty()) response.put("unanswered", new ArrayList<Integer>(calls.unanswered));
    response.put("logs", runtime == null ? new ArrayList<Object>() : runtime.logSnapshot());
    response.put("bindings", runtime == null ? new LinkedHashMap<String, Object>() : runtime.bindings);
    List<String> absent = runtime == null ? new ArrayList<String>() : new ArrayList<String>(runtime.missing);
    if (calls != null) for (String name : calls.missing) if (!absent.contains(name)) absent.add(name);
    for (String name : factory.missing) if (!absent.contains(name)) absent.add(name);
    if (factory.failure != null) {
      List<String> names = Json.optionalStringList(factory.failure.info, "missing");
      if (names != null) for (String name : names) if (!absent.contains(name)) absent.add(name);
    }
    if (Boolean.TRUE.equals(job.get("reportMissing")) || !absent.isEmpty()) response.put("missing", absent);
    java.util.Set<String> measurements = new java.util.TreeSet<String>();
    java.util.Set<String> assumptions = new java.util.TreeSet<String>();
    if (factory.usedSpawn) {
      measurements.add("spawn child settings and throwing-child join (orchestrator p20, 2026-11-02)");
      runtime.assumed.add("spawn uses harness observer threshold 1000 and a custom context factory for deadlines");
    }
    if (runtime != null) assumptions.addAll(runtime.assumed);
    java.util.List<Object> hostEvidence = calls == null ? new ArrayList<Object>() : calls.evidenceSnapshot();
    if (!hostEvidence.isEmpty()) response.put("provenance", hostEvidence);
    for (Object raw : hostEvidence) {
      Map<String, Object> entry = Json.asObject(raw, "host evidence");
      if ("assumed".equals(entry.get("kind"))) assumptions.add(Json.optionalString(entry, "behavior"));
      if ("measured".equals(entry.get("kind"))) measurements.add(Json.optionalString(entry, "behavior"));
    }
    if (!assumptions.isEmpty()) response.put("assumed", new ArrayList<String>(assumptions));
    if (!measurements.isEmpty()) response.put("measured", new ArrayList<String>(measurements));
    Map<String, Object> serialized = Json.asObject(IdmValues.wireValue(response), "response");
    if (Boolean.TRUE.equals(job.get("internalValues"))) {
      if ("ok".equals(response.get("outcome"))) serialized.put("internalValue", internalResult);
      serialized.put("bindings", runtime == null ? new LinkedHashMap<String, Object>() : IdmValues.snapshot(runtime.bindings));
    }
    return serialized;
  }

  private static Map<String, Object> thrown(RhinoException e) {
    Map<String, Object> result = new LinkedHashMap<String, Object>();
    if (e instanceof WrappedException) {
      Throwable cause = ((WrappedException) e).getWrappedException();
      result.put("kind", "javaException");
      result.put("class", cause.getClass().getName());
      result.put("message", cause.getMessage());
      if (cause instanceof HostCalls.HostException) {
        HostCalls.HostException host = (HostCalls.HostException) cause;
        result.put("class", host.exceptionClass());
        result.put("code", host.getCode());
        result.put("detail", host.detail());
      }
    } else if (e instanceof JavaScriptException) {
      Object value = ((JavaScriptException) e).getValue();
      result.put("kind", value instanceof Scriptable ? ("Error".equals(((Scriptable) value).getClassName()) ? "error" : "object")
          : value instanceof CharSequence ? "string" : "primitive");
      result.put("value", IdmValues.javaValue(value));
      result.put("message", Context.toString(value));
      if (value instanceof Scriptable) {
        Object javaException = ScriptableObject.getProperty((Scriptable) value, "javaException");
        if (javaException instanceof org.mozilla.javascript.Wrapper
            && ((org.mozilla.javascript.Wrapper) javaException).unwrap() instanceof HostCalls.HostException) {
          HostCalls.HostException host = (HostCalls.HostException) ((org.mozilla.javascript.Wrapper) javaException).unwrap();
          result.put("kind", "javaException");
          result.put("class", host.exceptionClass());
          result.put("message", host.getMessage());
          result.put("code", host.getCode());
          result.put("detail", host.detail());
          result.remove("value");
          return result;
        }
      }
      if (value instanceof Scriptable) {
        Scriptable obj = (Scriptable) value;
        for (String key : new String[] {"code", "message", "detail"}) {
          Object member = ScriptableObject.getProperty(obj, key);
          if (member != Scriptable.NOT_FOUND && (!"message".equals(key) || !"error".equals(result.get("kind")))) result.put(key, IdmValues.javaValue(member));
        }
      }
    } else {
      result.put("kind", "error");
      result.put("class", e instanceof EcmaError ? ((EcmaError) e).getName() : e.getClass().getName());
      result.put("message", e.details());
    }
    return result;
  }

  private void install() {
    Map<String, Object> openidm = new LinkedHashMap<String, Object>();
    for (String method : new String[] {"read", "create", "update", "patch", "delete", "query", "action"}) {
      openidm.put(method, function(method, args -> routed(method, args)));
    }
    for (String method : new String[] {"parseFilter", "hash", "encrypt", "isHashed", "matches", "decrypt", "isEncrypted"}) {
      openidm.put(method, function(method, args -> { throw missing("openidm." + method); }));
    }
    bind("openidm", surface(openidm));
    Map<String, Object> logger = new LinkedHashMap<String, Object>();
    for (String method : new String[] {"trace", "debug", "info", "warn", "error"}) {
      logger.put(method, function(method, args -> {
        if (args.length == 0 || !(args[0] instanceof CharSequence)) throw noMethod(method, args);
        log("logger", method, args);
        return Undefined.instance;
      }));
    }
    bind("logger", surface(logger));
    Map<String, Object> console = new LinkedHashMap<String, Object>();
    console.put("log", function("log", args -> { log("console", "log", args); return Undefined.instance; }));
    bind("console", surface(console));
    Map<String, Object> identity = new LinkedHashMap<String, Object>();
    identity.put("getProperty", function("getProperty", args -> {
      if (args.length < 1 || args.length > 3 || !(args[0] instanceof CharSequence)
          || (args.length == 3 && !(args[2] instanceof Boolean))) throw noMethod("getProperty", args);
      String name = args[0].toString();
      if (properties.containsKey(name)) return values.wrap(properties.get(name));
      String assumption = "property " + name + " absent";
      if (!assumed.contains(assumption)) assumed.add(assumption);
      return args.length > 1 ? values.wrap(IdmValues.javaValue(args[1])) : null;
    }));
    bind("identityServer", surface(identity));
    if (hostRequire) {
      BaseFunction[] require = new BaseFunction[1];
      require[0] = function("require", args -> requireModule(require[0], args));
      bind("require", require[0]);
    } else {
      bind("require", function("require", args -> { throw missing("require"); }));
    }
    bind("btoa", function("btoa", args -> java.util.Base64.getEncoder().encodeToString(Context.toString(args[0]).getBytes(java.nio.charset.StandardCharsets.ISO_8859_1))));
    bind("atob", function("atob", args -> new String(java.util.Base64.getDecoder().decode(Context.toString(args[0])), java.nio.charset.StandardCharsets.ISO_8859_1)));
    bind("secureRandom", Context.javaToJS(new java.security.SecureRandom(), scope));
  }

  private Object routed(String method, Object[] original) {
    Function callback = original.length > 0 && original[original.length - 1] instanceof Function ? (Function) original[original.length - 1] : null;
    List<Object> args = new ArrayList<Object>();
    String assumption = "internal openidm special-number transport preserves numeric values; tenant behavior unmeasured";
    if (!assumed.contains(assumption)) assumed.add(assumption);
    for (Object arg : original) if (!(arg instanceof Function)) args.add(IdmValues.internalValue(arg));
    int min = "read".equals(method) ? 1 : "query".equals(method) || "delete".equals(method) ? 2 : 3;
    int max = "read".equals(method) || "query".equals(method) || "delete".equals(method) ? 3 : "update".equals(method) || "patch".equals(method) ? 4 : 5;
    boolean valid = args.size() >= min && args.size() <= max && args.get(0) instanceof String;
    if (valid) {
      if ("action".equals(method)) valid = args.get(1) instanceof String && nullableMap(args.get(2)) && (args.size() < 4 || args.get(3) instanceof Map);
      else if ("create".equals(method) || "update".equals(method) || "patch".equals(method) || "delete".equals(method)) {
        valid = args.get(1) == null || args.get(1) instanceof String;
        if (args.size() >= 3) valid &= "patch".equals(method) ? args.get(2) instanceof List : "delete".equals(method) ? nullableMap(args.get(2)) : nullableMap(args.get(2));
      } else if (args.size() > 1) valid = nullableMap(args.get(1));
    }
    if (!valid) throw noMethod(method, original);
    Object reply;
    try {
      reply = calls.call(method, args);
    } catch (WrappedException e) {
      if (!(e.getWrappedException() instanceof HostCalls.HostException)) throw e;
      HostCalls.HostException host = (HostCalls.HostException) e.getWrappedException();
      Scriptable error = org.mozilla.javascript.ScriptRuntime.wrapException(e, scope, Context.getCurrentContext());
      ScriptableObject.putProperty(error, "message", host.toString());
      ((ScriptableObject) error).setAttributes("name", ScriptableObject.DONTENUM);
      throw new JavaScriptException(error, e.sourceName(), e.lineNumber());
    }
    if ("query".equals(method) && callback != null && reply instanceof Map) {
      Map<String, Object> query = new LinkedHashMap<String, Object>(Json.asObject(reply, "query response"));
      Object rows = query.remove("result");
      query.remove("resultCount");
      if (rows instanceof List) {
        for (Object row : (List<?>) rows) callback.call(Context.getCurrentContext(), scope, scope, new Object[] {values.wrap(row)});
      }
      reply = query;
    }
    return values.wrap(reply);
  }

  /**
   * CommonJS require over the host's runtime-script provider (E15). The module
   * runs in its own scope whose prototype is the caller's top scope, so it sees
   * the caller's bindings and keeps its top-level vars (IDM 9.0.0's ModuleScope,
   * read from bytecode). Exports are cached for this invocation only; a module
   * that throws is not cached. The host answers {path, source} or fails the job
   * with the idm-runtime-script fault.
   */
  private Object requireModule(BaseFunction require, Object[] args) {
    String id = args.length > 0 ? Context.toString(args[0]) : "undefined";
    String path = id.endsWith(".js") ? id : id + ".js";
    Scriptable cached = modules.get(path);
    if (cached != null) return ScriptableObject.getProperty(cached, "exports");
    List<Object> callArgs = new ArrayList<Object>();
    callArgs.add(id);
    Map<String, Object> loaded = Json.asObject(calls.call("require", callArgs), "require reply");
    if (!path.equals(loaded.get("path"))) throw calls.fail(new EmulationFailure("protocol", "rhino-local: require reply names " + loaded.get("path") + ", expected " + path));
    String text = Json.asString(loaded.get("source"), "require source");
    Context cx = Context.getCurrentContext();
    org.mozilla.javascript.NativeObject moduleScope = new org.mozilla.javascript.NativeObject();
    moduleScope.setPrototype(scope);
    moduleScope.setParentScope(null);
    Scriptable exports = cx.newObject(scope);
    Scriptable module = cx.newObject(scope);
    ScriptableObject.putProperty(module, "id", id);
    ScriptableObject.putProperty(module, "exports", exports);
    ScriptableObject.putProperty(moduleScope, "exports", exports);
    ScriptableObject.putProperty(moduleScope, "module", module);
    ScriptableObject.putProperty(moduleScope, "require", require);
    org.mozilla.javascript.Script script;
    try {
      script = ScriptCache.compile(cx, "idm-module:" + ScriptCache.sha256(text), () -> text, path, 1);
    } catch (org.mozilla.javascript.EvaluatorException e) {
      throw org.mozilla.javascript.ScriptRuntime.constructError("SyntaxError", e.details(), e.sourceName(), e.lineNumber(), e.lineSource(), e.columnNumber());
    }
    // Cached before it runs, so a cycle sees the partial exports, as CommonJS does.
    modules.put(path, module);
    boolean done = false;
    try {
      script.exec(cx, moduleScope);
      done = true;
    } finally {
      if (!done) modules.remove(path);
    }
    return ScriptableObject.getProperty(module, "exports");
  }

  private static boolean nullableMap(Object value) { return value == null || value instanceof Map; }

  private List<Object> logSnapshot() {
    synchronized (logs) { return new ArrayList<Object>(logs); }
  }

  private void log(String surface, String level, Object[] args) {
    String message = args.length == 0 ? "" : Context.toString(args[0]);
    List<Object> converted = new ArrayList<Object>();
    for (Object arg : args) converted.add(IdmValues.snapshot(IdmValues.javaValue(arg)));
    if (args.length > 1) for (Object arg : converted) if (arg instanceof Map || arg instanceof List) {
      assumed.add("logs snapshot each argument at emission; concurrent argument graphs have per-container consistency");
    }
    for (int i = 1; i < args.length; i++) {
      String replacement = args[i] instanceof Scriptable ? String.valueOf(converted.get(i)) : Context.toString(args[i]);
      int at = message.indexOf("{}");
      if (at >= 0) message = message.substring(0, at) + replacement + message.substring(at + 2);
      else if ("console".equals(surface)) message += " " + replacement;
    }
    Map<String, Object> entry = new LinkedHashMap<String, Object>();
    entry.put("surface", surface);
    entry.put("level", level);
    entry.put("message", message);
    entry.put("args", converted);
    logs.add(entry);
  }

  private void bind(String name, Object value) { ScriptableObject.putProperty(scope.getPrototype(), name, value); }

  private Scriptable surface(Map<String, Object> methods) {
    return (Scriptable) values.wrap(methods);
  }

  private interface Body { Object call(Object[] args); }
  private BaseFunction function(String name, Body body) {
    return new BaseFunction(scope, ScriptableObject.getFunctionPrototype(scope)) {
      @Override public String getFunctionName() { return name; }
      @Override public Object call(Context cx, Scriptable s, Scriptable self, Object[] args) { return body.call(args); }
      @Override public Object getDefaultValue(Class<?> hint) {
        throw Context.reportRuntimeError("Primitive type expected (had ScriptableFunction instead)");
      }
    };
  }

  private EmulationFailure missing(String name) {
    // A script catch must not conceal a fidelity boundary from the emulator.
    if (!missing.contains(name)) missing.add(name);
    EmulationFailure error = new EmulationFailure("missing", "rhino-local: " + name + " is not emulated yet");
    error.info.put("missing", java.util.Collections.singletonList(name));
    return calls.fail(error);
  }

  private static WrappedException noMethod(String method, Object[] args) {
    List<String> types = new ArrayList<String>();
    for (Object arg : args) {
      if (arg instanceof Function) continue;
      Object value = IdmValues.javaValue(arg);
      types.add(value == null ? "null" : value instanceof Map ? "HashMap" : value instanceof List ? "ArrayList"
          : value instanceof Number ? (((Number) value).doubleValue() >= Integer.MIN_VALUE && ((Number) value).doubleValue() <= Integer.MAX_VALUE && ((Number) value).doubleValue() == Math.rint(((Number) value).doubleValue()) ? "Integer" : "Double") : value.getClass().getSimpleName());
    }
    return new WrappedException(new NoSuchMethodException("Method not found " + method + "(" + String.join(",", types) + ")"));
  }

  public static final class ResourceName {
    private final String path;
    ResourceName(String path) { this.path = path; }
    @Override public String toString() { return path; }
  }
}
