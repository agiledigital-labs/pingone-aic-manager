import java.util.ArrayList;
import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.Map;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Callable;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.EcmaError;
import org.mozilla.javascript.Function;
import org.mozilla.javascript.JavaScriptException;
import org.mozilla.javascript.NativeArray;
import org.mozilla.javascript.NativeJSON;
import org.mozilla.javascript.RhinoException;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;
import org.mozilla.javascript.WrappedException;
import org.mozilla.javascript.json.JsonParser;

/**
 * Opt-in AM bridge. Reuses IDM's host transport and sticky fatal state, not its evaluator.
 *
 * <p>The trusted journey preamble calls this function once per verb to obtain Java function
 * objects, and installs those as the binding methods. Nothing author code can reach holds the
 * bridge: each returned function keeps it in a Java field, and its {@code __parent__} is the
 * global scope. So a refused call, a host call and the harvest cannot be redirected by rebinding
 * a closure variable, a global name or a built-in such as {@code JSON} or {@code
 * Function.prototype.apply}. Runner deletes the bridge's own global before author code runs.
 */
final class AmHostBridge extends BaseFunction {
  final IdmContextFactory state;
  final HostCalls calls;
  private Scriptable top;
  private Function errorType;
  private Callable harvest;
  private Scriptable runtime;
  private Scriptable action;
  private String errorMessage;

  AmHostBridge(String id, long timeoutMs) {
    state = new IdmContextFactory(timeoutMs, "AM harness timeout");
    calls = new HostCalls(id, state);
  }

  EmulationFailure missing(String behavior) {
    EmulationFailure failure =
        new EmulationFailure("missing", "Missing journey behavior: " + behavior);
    failure.info.put("missing", Collections.singletonList(behavior));
    return state.fail(failure);
  }

  private EmulationFailure protocol(String message) {
    return state.fail(new EmulationFailure("protocol", message));
  }

  private <T extends BaseFunction> T expose(T function) {
    function.setParentScope(top);
    function.setPrototype(ScriptableObject.getFunctionPrototype(top));
    return function;
  }

  private static Object arg(Object[] args, int index) {
    return index < args.length ? args[index] : Undefined.instance;
  }

  @Override
  public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
    String verb = Context.toString(arg(args, 0));
    if ("missing".equals(verb)) throw missing(Context.toString(arg(args, 1)));
    if ("install".equals(verb)) {
      if (top != null) throw protocol("journey bridge installed twice");
      if (!(arg(args, 1) instanceof Callable)
          || !(arg(args, 2) instanceof Scriptable)
          || !(arg(args, 3) instanceof Scriptable)) {
        throw protocol("journey bridge install arguments");
      }
      top = ScriptableObject.getTopLevelScope(scope);
      Object error = ScriptableObject.getProperty(top, "Error");
      if (!(error instanceof Function)) throw protocol("journey bridge: Error constructor");
      errorType = (Function) error;
      harvest = (Callable) args[1];
      runtime = (Scriptable) args[2];
      action = (Scriptable) args[3];
      return Undefined.instance;
    }
    if (top == null) throw protocol("journey bridge used before install");
    switch (verb) {
      case "refusal":
        return expose(new Refusal(Context.toString(arg(args, 1))));
      case "notMocked":
        return expose(new NotMocked());
      case "method":
        return expose(new HostMethod(Context.toString(arg(args, 1))));
      case "http":
        return expose(new HttpSend());
      case "errorMessage":
        return expose(new ErrorMessage());
      case "chain":
        return expose(new Chain());
      case "guard":
        if (!(arg(args, 1) instanceof Scriptable) || !(arg(args, 2) instanceof Callable)) {
          throw protocol("journey bridge guard arguments");
        }
        return expose(
            new Guard((Scriptable) args[1], (Callable) args[2], Context.toString(arg(args, 3))));
      default:
        throw protocol("journey bridge verb: " + verb);
    }
  }

  /** A binding method the emulator does not implement. Uncatchable by the script. */
  private final class Refusal extends BaseFunction {
    private final String behavior;

    Refusal(String behavior) {
      this.behavior = behavior;
    }

    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      throw missing(behavior);
    }
  }

  /** Replacement for the generated stubs' global hook, kept for any caller that names it. */
  private final class NotMocked extends BaseFunction {
    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      throw missing(Context.toString(arg(args, 0)) + "." + Context.toString(arg(args, 1)));
    }
  }

  /** The arguments array, as JSON, refusing values JSON would silently rewrite. */
  private Object encode(Context cx, Object[] values) {
    BaseFunction replacer =
        new BaseFunction() {
          @Override
          public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
            Object value = arg(args, 1);
            if (value instanceof Function) throw missing("AM host function arguments");
            if (value instanceof Number) {
              double number = ((Number) value).doubleValue();
              if (Double.isNaN(number)
                  || Double.isInfinite(number)
                  || (number == 0 && 1 / number < 0)) {
                throw missing("AM host non-finite or negative-zero arguments");
              }
            }
            return value;
          }
        };
    expose(replacer);
    Object text =
        NativeJSON.stringify(cx, top, cx.newArray(top, values), replacer, Undefined.instance);
    return Json.parse(Context.toString(text));
  }

  private Object toJs(Context cx, Object value) {
    try {
      return new JsonParser(cx, top).parseValue(Json.stringify(value));
    } catch (JsonParser.ParseException e) {
      throw protocol("AM host result is not JSON: " + e.getMessage());
    }
  }

  /** One host round trip; a tenant-shaped host error becomes a catchable JavaException. */
  private Object invoke(Context cx, String method, Object[] values) {
    Object input = encode(cx, values);
    Object result;
    try {
      result = calls.call(method, input);
    } catch (WrappedException e) {
      if (!(e.getWrappedException() instanceof HostCalls.HostException)) throw e;
      HostCalls.HostException host = (HostCalls.HostException) e.getWrappedException();
      Scriptable error = errorType.construct(cx, top, new Object[] {host.getMessage()});
      ScriptableObject.putProperty(error, "name", "JavaException");
      ScriptableObject.putProperty(error, "code", String.valueOf(host.getCode()));
      ScriptableObject.putProperty(error, "detail", toJs(cx, host.detail()));
      throw new JavaScriptException(error, null, 0);
    }
    return toJs(cx, result);
  }

  private final class HostMethod extends BaseFunction {
    private final String method;

    HostMethod(String method) {
      this.method = method;
    }

    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      return invoke(cx, method, args);
    }
  }

  /** Returns a fixed value; backs the response's json() and text(). */
  private final class Constant extends BaseFunction {
    private final Object value;

    Constant(Object value) {
      this.value = value;
    }

    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      return value;
    }
  }

  private final class HttpSend extends BaseFunction {
    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      Object options = arg(args, 1);
      if (!ScriptableObject.isTrue(options)) options = cx.newObject(top);
      Object reply =
          invoke(cx, "http", new Object[] {Context.toString(arg(args, 0)), options});
      if (!(reply instanceof Scriptable)) throw protocol("journey HTTP reply shape");
      Scriptable shape = (Scriptable) reply;
      Object status = ScriptableObject.getProperty(shape, "status");
      Object body = ScriptableObject.getProperty(shape, "body");
      Object headers = ScriptableObject.getProperty(shape, "headers");
      double code = Context.toNumber(status);
      Scriptable response = cx.newObject(top);
      ScriptableObject.putProperty(response, "status", status);
      ScriptableObject.putProperty(response, "ok", code >= 200 && code < 300);
      ScriptableObject.putProperty(
          response, "headers", ScriptableObject.isTrue(headers) ? headers : cx.newObject(top));
      ScriptableObject.putProperty(response, "statusText", Context.toString(status));
      ScriptableObject.putProperty(response, "json", expose(new Constant(body)));
      Object text =
          body instanceof CharSequence
              ? body
              : NativeJSON.stringify(cx, top, body, null, Undefined.instance);
      ScriptableObject.putProperty(response, "text", expose(new Constant(text)));
      Scriptable future = cx.newObject(top);
      ScriptableObject.putProperty(future, "get", expose(new Constant(response)));
      return future;
    }
  }

  /** action.withErrorMessage: the message is held here, not in a script-visible global. */
  private final class ErrorMessage extends BaseFunction {
    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      errorMessage = Context.toString(arg(args, 0));
      return action;
    }
  }

  /** action.withLockoutMessage: measured to have no visible effect. */
  private final class Chain extends BaseFunction {
    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      return action;
    }
  }

  /** A mock runtime method whose rhino-local diagnostics are unsupported emulation. */
  private final class Guard extends BaseFunction {
    private final Scriptable target;
    private final Callable original;
    private final String behavior;

    Guard(Scriptable target, Callable original, String behavior) {
      this.target = target;
      this.original = original;
      this.behavior = behavior;
    }

    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      try {
        return original.call(cx, scope, target, args);
      } catch (JavaScriptException e) {
        String text;
        try {
          text = Context.toString(e.getValue());
        } catch (RuntimeException unreadable) {
          throw e;
        }
        if (text.contains("rhino-local:")) throw missing(behavior + ": " + text);
        throw e;
      } catch (EcmaError e) {
        String text = e.getName() + ": " + e.getErrorMessage();
        if (text.contains("rhino-local:")) throw missing(behavior + ": " + text);
        throw e;
      }
    }
  }

  /**
   * After author code: refuse state values whose JSON form would differ from the value, then
   * harvest. Failures here are emulation failures, never script exceptions.
   */
  String harvest(Context cx) {
    if (top == null) throw protocol("journey bridge used before install");
    try {
      return harvestChecked(cx);
    } catch (RhinoException e) {
      throw missing("journey harvest serialization: " + e.details());
    }
  }

  private String harvestChecked(Context cx) {
    for (String bucket : new String[] {"shared", "transient"}) {
      Object root = ScriptableObject.getProperty(runtime, bucket);
      if (!(root instanceof ScriptableObject)) {
        throw missing("journey state " + bucket + " container");
      }
      ScriptableObject object = (ScriptableObject) root;
      IdentityHashMap<Object, Boolean> active = new IdentityHashMap<Object, Boolean>();
      active.put(object, Boolean.TRUE);
      for (Object id : object.getIds()) {
        checkState(bucket, key(id), get(object, id), active);
      }
    }
    Object effects = harvest.call(cx, top, top, new Object[0]);
    if (!(effects instanceof CharSequence)) throw missing("journey harvest serialization");
    return "{\"effects\":"
        + effects
        + ",\"errorMessage\":"
        + (errorMessage == null ? "null" : Json.quote(errorMessage))
        + "}";
  }

  private static String key(Object id) {
    return String.valueOf(id);
  }

  private static Object get(Scriptable object, Object id) {
    return id instanceof Integer
        ? ScriptableObject.getProperty(object, (Integer) id)
        : ScriptableObject.getProperty(object, String.valueOf(id));
  }

  private void checkState(
      String bucket, String path, Object value, IdentityHashMap<Object, Boolean> active) {
    String where = "journey state " + bucket + " value: ";
    if (value == null || value instanceof Boolean || value instanceof CharSequence) return;
    if (value instanceof Number) {
      double number = ((Number) value).doubleValue();
      if (Double.isNaN(number) || Double.isInfinite(number) || (number == 0 && 1 / number < 0)) {
        throw missing(where + "non-finite or negative-zero number at " + path);
      }
      return;
    }
    if (value == Undefined.instance || value == Scriptable.NOT_FOUND) {
      throw missing(where + "undefined at " + path);
    }
    if (value instanceof Function) throw missing(where + "function at " + path);
    if (!(value instanceof ScriptableObject)) throw missing(where + "host object at " + path);
    ScriptableObject object = (ScriptableObject) value;
    String type = object.getClassName();
    if (!"Object".equals(type) && !"Array".equals(type)) {
      throw missing(where + type + " object at " + path);
    }
    if (active.containsKey(object)) throw missing(where + "cyclic object at " + path);
    active.put(object, Boolean.TRUE);
    if (object instanceof NativeArray) {
      long length = ((NativeArray) object).getLength();
      for (long i = 0; i < length; i++) {
        checkState(bucket, path + "[" + i + "]", get(object, (int) i), active);
      }
    } else {
      for (Object id : object.getIds()) {
        checkState(bucket, path + "." + key(id), get(object, id), active);
      }
    }
    active.remove(object);
  }

  Map<String, Object> finish(String id, Map<String, Object> response) {
    calls.close();
    if (state.failure != null) {
      response =
          Runner.errorResponse(
              id,
              "timeout".equals(state.failure.info.get("kind")) ? "timeout" : "runtime_error",
              state.failure,
              null,
              -1,
              -1,
              null);
      Map<String, Object> fatal = new LinkedHashMap<String, Object>(state.failure.info);
      if (!state.missing.isEmpty()) fatal.put("missing", new ArrayList<String>(state.missing));
      response.put("emulation", fatal);
    }
    response.put("missing", new ArrayList<String>(state.missing));
    response.put("provenance", calls.evidenceSnapshot());
    if (!calls.unanswered.isEmpty()) {
      response.put("unanswered", new ArrayList<Integer>(calls.unanswered));
    }
    return response;
  }
}
