import java.util.ArrayList;
import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.Map;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Callable;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Function;
import org.mozilla.javascript.JavaScriptException;
import org.mozilla.javascript.NativeArray;
import org.mozilla.javascript.NativeJSON;
import org.mozilla.javascript.ScriptRuntime;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;
import org.mozilla.javascript.WrappedException;
import org.mozilla.javascript.Wrapper;
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
  private Scriptable objectProto;
  private Scriptable arrayProto;
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
      if (!(arg(args, 1) instanceof Scriptable) || !(arg(args, 2) instanceof Scriptable)) {
        throw protocol("journey bridge install arguments");
      }
      top = ScriptableObject.getTopLevelScope(scope);
      Object error = ScriptableObject.getProperty(top, "Error");
      if (!(error instanceof Function)) throw protocol("journey bridge: Error constructor");
      errorType = (Function) error;
      objectProto = ScriptableObject.getObjectPrototype(top);
      arrayProto = ScriptableObject.getArrayPrototype(top);
      runtime = (Scriptable) args[1];
      action = (Scriptable) args[2];
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
      case "unsupported":
        return expose(new Unsupported());
      case "observe":
        if (!(arg(args, 1) instanceof Callable)) throw protocol("journey bridge observe arguments");
        if (args[1] instanceof Refusal) return args[1];
        return expose(new Observe((Callable) args[1], Context.toString(arg(args, 2))));
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

  /**
   * Replaces the runtime's __rhinoLocalFail: every "rhino-local:" diagnostic is an approximation
   * the local runtime cannot stand behind, so it fails the run as missing. The decision is made
   * here, by which function was called, never by reading an error's text after a script could
   * have touched it.
   */
  private final class Unsupported extends BaseFunction {
    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      String message = Context.toString(arg(args, 0));
      throw missing(message.startsWith("rhino-local: ") ? message.substring(13) : message);
    }
  }

  /** A binding member measured in docs/api: runs the local implementation and says so. */
  private final class Observe extends BaseFunction {
    private final Callable original;
    private final String behavior;

    Observe(Callable original, String behavior) {
      this.original = original;
      this.behavior = behavior;
    }

    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      calls.note("measured", behavior + " runs locally as measured", UTILS_SOURCE);
      return original.call(cx, scope, thisObj, args);
    }
  }

  /** Replacement for the generated stubs' global hook, kept for any caller that names it. */
  private final class NotMocked extends BaseFunction {
    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      throw missing(Context.toString(arg(args, 0)) + "." + Context.toString(arg(args, 1)));
    }
  }

  private static final String UTILS_SOURCE =
      "docs/api/12-script-bindings-matrix.md (utils, utils.crypto.subtle)";
  private static final String BODY_SOURCE =
      "docs/api/12-script-bindings-matrix.md (httpClient.send body serialization)";
  private static final String NONJSON_SOURCE =
      "docs/api/12-script-bindings-matrix.md (§5 non-JSON values in an object body, 2026-11-04)";
  private static final String CREATE_SOURCE =
      "docs/api/12-script-bindings-matrix.md (openidm.create content with an undefined value,"
          + " 2026-11-04)";

  /** True when the parsed headers carry exactly the Content-Type every measurement sent. */
  private static boolean jsonContentType(Object headers) {
    if (!(headers instanceof Map)) return false;
    for (Map.Entry<?, ?> header : ((Map<?, ?>) headers).entrySet()) {
      if ("content-type".equalsIgnoreCase(String.valueOf(header.getKey()))) {
        return "application/json".equals(header.getValue());
      }
    }
    return false;
  }
  private static final java.util.Set<String> MAP_HIDDEN =
      new java.util.HashSet<String>(
          java.util.Arrays.asList(
              "__rhinoLocalMapEntries",
              "toJSON",
              "toString",
              "get",
              "put",
              "containsKey",
              "size",
              "keySet",
              "entrySet",
              "values"));
  private static final java.util.Set<String> LIST_HIDDEN =
      new java.util.HashSet<String>(
          java.util.Arrays.asList("size", "get", "isEmpty", "contains", "toArray", "iterator"));
  private static java.lang.reflect.Method descriptorMethod;

  /** ScriptableObject.getOwnPropertyDescriptor is protected; it is the one read that sees accessors. */
  private ScriptableObject descriptor(Context cx, ScriptableObject object, Object id) {
    try {
      if (descriptorMethod == null) {
        java.lang.reflect.Method method =
            ScriptableObject.class.getDeclaredMethod(
                "getOwnPropertyDescriptor", Context.class, Object.class);
        method.setAccessible(true);
        descriptorMethod = method;
      }
      return (ScriptableObject) descriptorMethod.invoke(object, cx, id);
    } catch (java.lang.reflect.InvocationTargetException e) {
      throw protocol("journey value descriptor: " + e.getCause());
    } catch (ReflectiveOperationException | RuntimeException e) {
      throw protocol("journey value descriptor: " + e);
    }
  }

  /**
   * Serializes a script value to JSON text in Java, from the value itself: no toJSON, no global
   * JSON, no script-reachable function runs. Anything whose JSON form would not be the value — a
   * custom prototype, an accessor, a hidden or array-named property, a hole, a cycle, a non-finite
   * number, undefined, a function, a host object — is missing, never rewritten or thrown as a
   * catchable error. The exception is an httpClient.send body, where docs/api/12 §5 measured what
   * AIC does with non-JSON values and the encoder does the same: NaN and ±Infinity become strings,
   * -0 is -0.0, a java.util.Date is its epoch milliseconds, and a cycle of plain objects throws
   * AIC's catchable InternalError. A JS Date, RegExp or function, which AIC sends as a dump of the
   * Rhino object, is missing with a message saying so.
   */
  private final class Encoder {
    private final Context cx;
    private final String where;
    private final String nullUnder;
    private final String nullFieldsOf;
    private final boolean wire;
    private final StringBuilder out = new StringBuilder();
    private final IdentityHashMap<Object, Boolean> active = new IdentityHashMap<Object, Boolean>();
    /** Set when undefined was written as null under nullUnder, or as a null field of nullFieldsOf. */
    boolean nulled;
    boolean fieldNulled;
    /** Why wire text may differ from AIC's: each names a shape docs/api/12 did not measure. */
    final java.util.TreeSet<String> assumed = new java.util.TreeSet<String>();
    /** docs/api/12 §5 conversions this body used, each a measured provenance note. */
    final java.util.TreeSet<String> measured = new java.util.TreeSet<String>();

    Encoder(Context cx, String where, String nullUnder) {
      this(cx, where, nullUnder, null, false);
    }

    /**
     * nullUnder: undefined at or below this path is written as null (measured for HTTP bodies).
     * nullFieldsOf: an undefined field of the object at exactly this path is written as null
     * (measured for openidm.create content). wire: numbers are rendered as AIC's httpClient writes
     * them.
     */
    Encoder(Context cx, String where, String nullUnder, String nullFieldsOf, boolean wire) {
      this.cx = cx;
      this.where = where;
      this.nullUnder = nullUnder;
      this.nullFieldsOf = nullFieldsOf;
      this.wire = wire;
    }

    private boolean under(String path) {
      return nullUnder.isEmpty()
          || path.equals(nullUnder)
          || path.startsWith(nullUnder + ".")
          || path.startsWith(nullUnder + "[");
    }

    /**
     * docs/api/12: every JS number goes out as a Java double (1 is 1.0, measured for integers up
     * to 1e6 at any depth); a boxed java.lang.Integer or Long goes out as written. Anything else
     * is rendered the same way but flagged, since its form on the wire was not measured.
     */
    private String wireNumber(Object value, double number, boolean wrapped) {
      if (wrapped && (value instanceof Integer || value instanceof Long)) return value.toString();
      if (!wrapped && value instanceof Integer) {
        assumed.add(
            "httpClient body text for a number the script engine holds as a Java Integer (results"
                + " of length, indexOf, JSON.parse), written as a Java double");
      } else if (!(value instanceof Double)) {
        assumed.add(
            "httpClient body text for a number held as a Java "
                + value.getClass().getSimpleName());
      } else if (number != Math.rint(number) || Math.abs(number) > 1e6) {
        assumed.add(
            "httpClient body text for a fractional number or an integer past 1e6, written as a"
                + " Java double");
      }
      return wrapped && !(value instanceof Double) ? value.toString() : Double.toString(number);
    }

    /**
     * docs/api/12 §4 (2026-11-04), exact: in printable ASCII only {@code "} and {@code \\} are
     * escaped (no {@code \/}); other BMP characters go out raw (é, U+00A0, U+2028, U+FEFF, U+007F
     * measured); {@code \n}/{@code \t} short; other C0 controls as backslash-u00XX in upper-case
     * hex; a surrogate pair, and a lone high surrogate, as upper-case backslash-u escapes. Keys go
     * through here too, as measured. What §4 did not sample is written the same way and flagged.
     */
    private String wireString(String value) {
      StringBuilder out = new StringBuilder(value.length() + 2).append('"');
      for (int i = 0; i < value.length(); i++) {
        char c = value.charAt(i);
        if (c == '"' || c == '\\') out.append('\\').append(c);
        else if (c == '\n') out.append("\\n");
        else if (c == '\t') out.append("\\t");
        else if (c == '\r' || c == '\b' || c == '\f') {
          assumed.add(
              "httpClient body text for \\r, \\b or \\f in a string, written as a short escape"
                  + " (docs/api/12 §4 measured only \\n and \\t)");
          out.append(c == '\r' ? "\\r" : c == '\b' ? "\\b" : "\\f");
        } else if (c < 0x20 || Character.isSurrogate(c)) {
          if (Character.isLowSurrogate(c)
              && !(i > 0 && Character.isHighSurrogate(value.charAt(i - 1)))) {
            assumed.add(
                "httpClient body text for a lone low surrogate, written as an upper-case escape like"
                    + " the measured lone high surrogate");
          }
          out.append(String.format(java.util.Locale.ROOT, "\\u%04X", (int) c));
        } else {
          if (c >= 0x80 && c <= 0x9f) {
            assumed.add(
                "httpClient body text for a C1 control character (U+0080-U+009F), written raw like"
                    + " the measured U+007F and other non-ASCII");
          }
          out.append(c);
        }
      }
      return out.append('"').toString();
    }

    /** True inside an httpClient.send body, where docs/api/12 §5 applies. */
    private boolean inBody(String path) {
      return nullUnder != null && under(path);
    }

    /** The body-relative path a script author recognises: body, body.d, body[0]. */
    private String bodyPath(String path) {
      String rel = path.substring(nullUnder.length());
      if (!rel.isEmpty() && rel.charAt(0) != '.' && rel.charAt(0) != '[') rel = "." + rel;
      return "body" + rel;
    }

    /** A §5 conversion: measured under an object key; inside an array it is the same guess. */
    private void converted(String behavior, String path) {
      measured.add(behavior);
      if (bodyPath(path).indexOf('[') >= 0) {
        assumed.add(
            "httpClient body text for NaN, ±Infinity, -0 or a java.util.Date inside an array,"
                + " written as measured under an object key (docs/api/12 §5)");
      }
    }

    /** §5: AIC sends a dump of the Rhino object, which nothing here can reproduce. */
    private EmulationFailure dump(String what, String path) {
      return missing("httpClient.send " + bodyPath(path) + " is " + what + " (docs/api/12 §5)");
    }

    private EmulationFailure shape(String what, String path) {
      return missing(where + what + (path.isEmpty() ? "" : " at " + path));
    }

    private String child(String path, Object id) {
      return path.isEmpty() ? String.valueOf(id) : path + "." + id;
    }

    String text() {
      return out.toString();
    }

    Encoder write(Object value, String path) {
      boolean wrapped = false;
      Object unwrapped = value instanceof Wrapper ? ((Wrapper) value).unwrap() : value;
      if (unwrapped != null && unwrapped.getClass() == java.util.Date.class && inBody(path)) {
        converted("httpClient.send body java.util.Date is sent as its epoch milliseconds", path);
        out.append(Long.toString(((java.util.Date) unwrapped).getTime()));
        return this;
      }
      if (value instanceof ScriptableObject && inBody(path)) {
        String type = ((ScriptableObject) value).getClassName();
        if ("Date".equals(type)) {
          throw dump(
              "a JS Date: AIC sends a ~1 KB dump of the Rhino NativeDate object without the time"
                  + " value, not the date; send date.toISOString() or date.getTime()",
              path);
        }
        if ("RegExp".equals(type)) {
          throw dump(
              "a RegExp: AIC sends a ~1 KB dump of the Rhino NativeRegExp object without its source"
                  + " or flags; send regexp.source as a string",
              path);
        }
        if (value instanceof Function) {
          throw dump(
              "a function: AIC sends a ~1.7 KB dump of the Rhino function object, which exposes the"
                  + " script's name, its encoded source and the name of every binding and variable"
                  + " in the script; leave it out of the body",
              path);
        }
      }
      if (value instanceof Wrapper) {
        Object raw = ((Wrapper) value).unwrap();
        if (!(raw instanceof CharSequence || raw instanceof Boolean || raw instanceof Number)) {
          throw shape("host object", path);
        }
        value = raw;
        wrapped = true;
      }
      if (value == null) {
        out.append("null");
      } else if (value == Undefined.instance || value == Scriptable.NOT_FOUND) {
        if (nullUnder == null || !under(path)) throw shape("undefined", path);
        nulled = true;
        out.append("null");
      } else if (value instanceof Boolean) {
        out.append(value.toString());
      } else if (value instanceof CharSequence) {
        out.append(wire ? wireString(value.toString()) : Json.quote(value.toString()));
      } else if (value instanceof Number) {
        double number = ((Number) value).doubleValue();
        boolean nonFinite = Double.isNaN(number) || Double.isInfinite(number);
        boolean negativeZero = number == 0 && 1 / number < 0;
        if ((nonFinite || negativeZero) && !wrapped && inBody(path)) {
          if (negativeZero) {
            converted("httpClient.send body -0 is sent as -0.0", path);
            out.append("-0.0");
          } else {
            converted(
                "httpClient.send body NaN and ±Infinity are sent as the strings \"NaN\","
                    + " \"Infinity\" and \"-Infinity\"",
                path);
            String text = Double.isNaN(number) ? "NaN" : number > 0 ? "Infinity" : "-Infinity";
            out.append(wire ? wireString(text) : Json.quote(text));
          }
          return this;
        }
        if (nonFinite || negativeZero) {
          throw shape("non-finite or negative-zero number", path);
        }
        out.append(
            wire ? wireNumber(value, number, wrapped) : ScriptRuntime.numberToString(number, 10));
      } else if (value instanceof Function) {
        throw shape("function", path);
      } else if (!(value instanceof ScriptableObject)) {
        throw shape("host object", path);
      } else {
        object((ScriptableObject) value, path);
      }
      return this;
    }

    private void object(ScriptableObject object, String path) {
      boolean array = object instanceof NativeArray;
      String type = object.getClassName();
      if (!(array ? "Array".equals(type) : "Object".equals(type))) {
        throw shape(type + " object", path);
      }
      if (active.containsKey(object)) {
        if (inBody(path) && !array && bodyPath(path).indexOf('[') < 0) {
          // docs/api/12 §5: a cycle of plain objects fails the send with a catchable error.
          calls.note(
              "measured",
              "httpClient.send body with a cycle throws a catchable InternalError; nothing is sent",
              NONJSON_SOURCE);
          throw ScriptRuntime.constructError(
              "InternalError", "Cannot produce JSON from [object Object]");
        }
        throw shape("cyclic object", path);
      }
      if (object.getPrototype() != (array ? arrayProto : objectProto)) {
        throw shape("non-plain prototype", path);
      }
      active.put(object, Boolean.TRUE);
      java.util.Set<String> hidden =
          array
              ? LIST_HIDDEN
              : ownHidden(object, "__rhinoLocalMapEntries")
                  ? MAP_HIDDEN
                  : Collections.<String>emptySet();
      java.util.TreeMap<Integer, Object> elements = new java.util.TreeMap<Integer, Object>();
      LinkedHashMap<String, Object> fields = new LinkedHashMap<String, Object>();
      for (Object id : object.getAllIds()) {
        if (array && "length".equals(id)) continue;
        String at =
            array && id instanceof Integer ? path + "[" + id + "]" : child(path, id);
        ScriptableObject desc = descriptor(cx, object, id);
        if (desc == null) continue;
        if (desc.has("get", desc) || desc.has("set", desc)) throw shape("accessor property", at);
        Object item = desc.get("value", desc);
        if (!ScriptRuntime.toBoolean(desc.get("enumerable", desc))) {
          if (hidden.contains(String.valueOf(id)) && item instanceof Function) continue;
          throw shape("non-enumerable property", at);
        }
        if (array) {
          if (!(id instanceof Integer)) throw shape("array property", at);
          elements.put((Integer) id, item);
        } else if (item == Undefined.instance && path.equals(nullFieldsOf)) {
          fieldNulled = true;
          fields.put(String.valueOf(id), null);
        } else {
          fields.put(String.valueOf(id), item);
        }
      }
      if (array) {
        long length = ((NativeArray) object).getLength();
        if (elements.size() != length) throw shape("array hole", path);
        out.append('[');
        int index = 0;
        for (Map.Entry<Integer, Object> element : elements.entrySet()) {
          if (index++ > 0) out.append(',');
          write(element.getValue(), path + "[" + element.getKey() + "]");
        }
        out.append(']');
      } else {
        out.append('{');
        boolean first = true;
        for (Map.Entry<String, Object> field : fields.entrySet()) {
          if (!first) out.append(',');
          first = false;
          out.append(wire ? wireString(field.getKey()) : Json.quote(field.getKey())).append(':');
          write(field.getValue(), child(path, field.getKey()));
        }
        out.append('}');
      }
      active.remove(object);
    }

    private boolean ownHidden(ScriptableObject object, String name) {
      ScriptableObject desc = descriptor(cx, object, name);
      return desc != null
          && !ScriptRuntime.toBoolean(desc.get("enumerable", desc))
          && desc.get("value", desc) instanceof Function;
    }
  }

  /** The arguments array as JSON, refusing values JSON would silently rewrite. */
  private Object encode(
      Context cx, String label, Object[] values, String nullUnder, String nullFieldsOf) {
    Encoder encoder =
        new Encoder(cx, "AM host " + label + " argument: ", nullUnder, nullFieldsOf, false);
    encoder.out.append('[');
    for (int i = 0; i < values.length; i++) {
      if (i > 0) encoder.out.append(',');
      encoder.write(values[i], "[" + i + "]");
    }
    encoder.out.append(']');
    if (encoder.nulled) {
      calls.note("measured", "httpClient.send body undefined is sent as null", BODY_SOURCE);
    }
    if (encoder.fieldNulled) {
      calls.note(
          "measured", "openidm.create content field holding undefined is sent as null", CREATE_SOURCE);
    }
    return Json.parse(encoder.text());
  }

  private Object toJs(Context cx, Object value) {
    try {
      return new JsonParser(cx, top).parseValue(Json.stringify(value));
    } catch (JsonParser.ParseException e) {
      throw protocol("AM host result is not JSON: " + e.getMessage());
    }
  }

  /** One host round trip; a tenant-shaped host error becomes a catchable JavaException. */
  private Object invoke(Context cx, String method, Object input) {
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
      // docs/api/12 (2026-11-04): an undefined field of openidm.create content is sent as null,
      // so a schema default does not land. Deeper, and in other verbs, it stays missing.
      String fields = "create".equals(method) ? "[2]" : null;
      return invoke(cx, method, encode(cx, "openidm." + method, args, null, fields));
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
      Object url = arg(args, 0);
      if (url == Undefined.instance || url == null) throw missing("httpClient.send with no URL");
      Object options = arg(args, 1);
      if (!ScriptableObject.isTrue(options)) options = cx.newObject(top);
      // docs/api/12 (httpClient.send body serialization): undefined in a body goes out as null.
      Object input =
          encode(
              cx,
              "httpClient.send",
              new Object[] {Context.toString(url), options},
              "[1].body",
              null);
      // Passed the encoder above, so options is plain data with no accessor left to run.
      @SuppressWarnings("unchecked")
      java.util.List<Object> list = (java.util.List<Object>) input;
      @SuppressWarnings("unchecked")
      Map<String, Object> sent =
          list.get(1) instanceof Map
              ? (Map<String, Object>) list.get(1)
              : Collections.<String, Object>emptyMap();
      // docs/api/12: the form-encoded option was not probed, and its body is not the JSON text.
      if (sent.containsKey("form")) throw missing("httpClient.send form body text");
      Map<String, Object> wire = new LinkedHashMap<String, Object>();
      java.util.TreeSet<String> assumed = new java.util.TreeSet<String>();
      if (sent.containsKey("body")) {
        Object body = ScriptableObject.getProperty((ScriptableObject) options, "body");
        if (body instanceof Wrapper) body = ((Wrapper) body).unwrap();
        // Every docs/api/12 body measurement sent exactly this header.
        if (!jsonContentType(sent.get("headers"))) {
          assumed.add("httpClient body text without a Content-Type: application/json header");
        }
        if (body instanceof CharSequence) {
          wire.put("body", body.toString());
          assumed.add("httpClient body text for a string body, sent as the string itself");
        } else {
          Encoder text = new Encoder(cx, "AM host httpClient.send body: ", "", null, true);
          text.write(body, "");
          if (!(body == null
              || body == Undefined.instance
              || (body instanceof ScriptableObject
                  && "Object".equals(((ScriptableObject) body).getClassName())))) {
            assumed.add("httpClient body text for a top-level non-object body");
          }
          assumed.addAll(text.assumed);
          for (String behavior : text.measured) calls.note("measured", behavior, NONJSON_SOURCE);
          wire.put("body", text.text());
        }
      }
      wire.put("assumed", new ArrayList<Object>(assumed));
      list.add(wire);
      Object reply = invoke(cx, "http", input);
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

  /**
   * After author code: the node's effects, serialized in Java from the runtime object the trusted
   * preamble handed over. No global, built-in or toJSON is consulted, so author code cannot
   * redirect it short of rewriting that object itself. Failures here are emulation failures, never
   * script exceptions.
   */
  String harvest(Context cx) {
    if (top == null) throw protocol("journey bridge used before install");
    Object queued = ScriptableObject.getProperty(runtime, "callbacks");
    if (!(queued instanceof NativeArray)) throw missing("journey callbacks container");
    // Queued next-gen callbacks suspend the node even after goTo.
    Object outcome =
        ((NativeArray) queued).getLength() > 0
            ? null
            : ScriptableObject.getProperty(runtime, "outcome");
    if (outcome != null && !(outcome instanceof CharSequence)) {
      throw missing("journey outcome value");
    }
    StringBuilder out = new StringBuilder("{\"effects\":{\"outcome\":");
    out.append(outcome == null ? "null" : Json.quote(outcome.toString()));
    StringBuilder maps = new StringBuilder();
    for (String bucket : new String[] {"shared", "transient"}) {
      Object root = ScriptableObject.getProperty(runtime, bucket);
      if (!(root instanceof ScriptableObject) || root instanceof NativeArray) {
        throw missing("journey state " + bucket + " container");
      }
      out.append(",\"").append(bucket).append("State\":{\"final\":");
      out.append(new Encoder(cx, "journey state " + bucket + " value: ", null).write(root, "").text());
      out.append('}');
      Object attributes = ScriptableObject.getProperty((Scriptable) root, "objectAttributes");
      boolean map =
          attributes instanceof ScriptableObject
              && ((ScriptableObject) attributes).has("__rhinoLocalMapEntries", (Scriptable) attributes);
      maps.append(maps.length() == 0 ? "" : ",")
          .append('"')
          .append(bucket)
          .append("\":")
          .append(map);
    }
    out.append(",\"callbacks\":");
    out.append(new Encoder(cx, "journey callbacks value: ", null).write(queued, "callbacks").text());
    out.append(",\"javaMapObjectAttributes\":{").append(maps).append("}}");
    out.append(",\"errorMessage\":");
    out.append(errorMessage == null ? "null" : Json.quote(errorMessage));
    return out.append('}').toString();
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
