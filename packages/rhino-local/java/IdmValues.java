import java.util.ArrayList;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.TreeMap;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.NativeArray;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;
import org.mozilla.javascript.Wrapper;

/** Java-backed IDM values. Wire conversion and script wrappers share one backing graph. */
final class IdmValues {
  private final Scriptable scope;
  private final IdentityHashMap<Object, Scriptable> wrappers = new IdentityHashMap<Object, Scriptable>();

  IdmValues(Scriptable scope) { this.scope = scope; }


  synchronized Object wrap(Object value) {
    if (!(value instanceof Map) && !(value instanceof List)) return value;
    Scriptable cached = wrappers.get(value);
    if (cached != null) return cached;
    Value result = value instanceof Map ? new MapValue(Json.asObject(value, "map")) : new ListValue((List<?>) value);
    wrappers.put(value, result);
    return result;
  }

  /**
   * A caught exception's {@code getDetail()}: a container that renders as CREST's one-line
   * JsonValue form (measured 2026-11-08, docs/api/10), not the multi-line form other host values
   * use. Spellings the probes never showed fault under the owner's harness-fault id.
   */
  synchronized Object wrapDetail(Object detail, HostCalls.HostException owner) {
    if (!(detail instanceof Map) && !(detail instanceof List)) throw owner.unmeasured("getDetail() of a detail that is not an object or array");
    Value result = (Value) wrap(detail);
    result.detailOwner = owner;
    return result;
  }

  synchronized Object wrapRequest(Object value) {
    Map<String, Object> map = Json.asObject(value, "request");
    String method = Json.asString(map.get("method"), "request.method");
    String tag = "Scriptable" + Character.toUpperCase(method.charAt(0)) + method.substring(1) + "Request";
    MapValue result = new MapValue(map, tag);
    wrappers.put(map, result);
    return result;
  }

  static void prepareHttpHeaders(Object context) {
    if (!(context instanceof Map)) return;
    Object http = Json.asObject(context, "context").get("http");
    if (!(http instanceof Map)) return;
    Map<String, Object> httpMap = Json.asObject(http, "http");
    Object headers = httpMap.get("headers");
    if (!(headers instanceof Map)) return;
    Map<String, Object> insensitive = new TreeMap<String, Object>(String.CASE_INSENSITIVE_ORDER);
    insensitive.putAll(Json.asObject(headers, "headers"));
    httpMap.put("headers", insensitive);
  }

  static Object javaValue(Object value) {
    return javaValue(value, new IdentityHashMap<Object, Boolean>(), false);
  }

  static Object internalValue(Object value) {
    return snapshot(javaValue(value, new IdentityHashMap<Object, Boolean>(), true));
  }

  /** Copy each shared backing container under the same lock used by its wrapper.
   * Never hold a container lock across script getters, host calls or another graph. */
  static Object snapshot(Object value) { return snapshot(value, new IdentityHashMap<Object, Boolean>()); }
  private static Object snapshot(Object value, IdentityHashMap<Object, Boolean> active) {
    if (!(value instanceof Map) && !(value instanceof List)) return value;
    if (active.put(value, Boolean.TRUE) != null) throw Context.reportRuntimeError("rhino-local: cyclic IDM value");
    try {
      if (value instanceof Map) {
        Map<String, Object> copy;
        synchronized (value) { copy = new LinkedHashMap<String, Object>(Json.asObject(value, "map")); }
        for (Map.Entry<String, Object> e : copy.entrySet()) e.setValue(snapshot(e.getValue(), active));
        return copy;
      }
      List<Object> copy;
      synchronized (value) { copy = new ArrayList<Object>((List<?>) value); }
      for (int i = 0; i < copy.size(); i++) copy.set(i, snapshot(copy.get(i), active));
      return copy;
    } finally { active.remove(value); }
  }

  private static Object javaValue(Object value, IdentityHashMap<Object, Boolean> active, boolean internal) {
    if (value == Scriptable.NOT_FOUND || Undefined.isUndefined(value)) return null;
    if (value instanceof Value) return ((Value) value).backing();
    if (value instanceof Wrapper) return javaValue(((Wrapper) value).unwrap(), active, internal);
    if (value instanceof CharSequence) return value.toString();
    if (value instanceof Number) {
      double n = ((Number) value).doubleValue();
      if (internal && (!Double.isFinite(n) || Double.doubleToRawLongBits(n) == Double.doubleToRawLongBits(-0.0))) return Double.valueOf(n);
      if (Double.isNaN(n)) return Double.valueOf(n);
      if (n == Math.rint(n)) return Long.valueOf((long) n);
      return Double.valueOf(n);
    }
    if (!(value instanceof Scriptable)) return value;
    if (active.put(value, Boolean.TRUE) != null) throw Context.reportRuntimeError("rhino-local: cyclic IDM value");
    try {
      Scriptable scriptable = (Scriptable) value;
      if (value instanceof NativeArray) {
        List<Object> list = new ArrayList<Object>();
        long length = ((NativeArray) value).getLength();
        for (int i = 0; i < length; i++) list.add(javaValue(scriptable.get(i, scriptable), active, internal));
        return list;
      }
      Map<String, Object> map = new HashMap<String, Object>();
      for (Object id : scriptable.getIds()) {
        Object member = id instanceof Number ? scriptable.get(((Number) id).intValue(), scriptable)
            : scriptable.get(id.toString(), scriptable);
        map.put(id.toString(), javaValue(member, active, internal));
      }
      return map;
    } finally {
      active.remove(value);
    }
  }

  /** Endpoint/protocol conversion must not alter numeric values already stored in maps. */
  static Object wireValue(Object value) {
    value = snapshot(javaValue(value));
    if (value instanceof Number && Double.isNaN(((Number) value).doubleValue())) return "NaN";
    if (value instanceof Map) {
      Map<String, Object> result = new LinkedHashMap<String, Object>();
      for (Map.Entry<String, Object> e : Json.asObject(value, "map").entrySet()) result.put(e.getKey(), wireValue(e.getValue()));
      return result;
    }
    if (value instanceof List) {
      List<Object> result = new ArrayList<Object>();
      for (Object item : (List<?>) value) result.add(wireValue(item));
      return result;
    }
    return value;
  }

  private Object nativeValue(Object value) {
    Context cx = Context.getCurrentContext();
    if (value instanceof Map) {
      Scriptable obj = cx.newObject(scope);
      for (Map.Entry<String, Object> e : Json.asObject(value, "map").entrySet()) {
        ScriptableObject.putProperty(obj, e.getKey(), nativeValue(e.getValue()));
      }
      return obj;
    }
    if (value instanceof List) {
      List<?> list = (List<?>) value;
      Object[] items = new Object[list.size()];
      for (int i = 0; i < items.length; i++) items[i] = nativeValue(list.get(i));
      return cx.newArray(scope, items);
    }
    return value;
  }

  private abstract class Value extends ScriptableObject {
    private volatile Scriptable prototype;
    private volatile Scriptable parent = scope;
    Value(boolean array) {
      prototype = array ? ScriptableObject.getArrayPrototype(scope) : ScriptableObject.getObjectPrototype(scope);
    }
    abstract Object backing();
    @Override public Object[] getAllIds() { return new Object[0]; }
    @Override protected ScriptableObject getOwnPropertyDescriptor(Context cx, Object id) { return null; }
    @Override public Scriptable getPrototype() { return prototype; }
    @Override public void setPrototype(Scriptable value) { prototype = value; }
    @Override public Scriptable getParentScope() { return parent; }
    @Override public void setParentScope(Scriptable value) { parent = value; }
    @Override public boolean hasInstance(Scriptable instance) { return false; }
    /** Set for an exception's detail: renders one-line, faulting through this owner. */
    volatile HostCalls.HostException detailOwner;
    @Override public Object getDefaultValue(Class<?> hint) {
      HostCalls.HostException owner = detailOwner;
      return owner == null ? pretty(backing()) : crest(wireValue(backing()), owner);
    }
  }

  /** CREST JsonValue's one-line form, as far as the captured details spell it. */
  static String crest(Object value, HostCalls.HostException owner) {
    if (value instanceof Map) {
      Map<String, Object> map = Json.asObject(value, "detail");
      if (map.isEmpty()) throw owner.unmeasured("the string form of an empty object in getDetail()");
      StringBuilder out = new StringBuilder("{ ");
      String separator = "";
      for (Map.Entry<String, Object> entry : map.entrySet()) {
        out.append(separator).append(crest(entry.getKey(), owner)).append(": ").append(crest(entry.getValue(), owner));
        separator = ", ";
      }
      return out.append(" }").toString();
    }
    if (value instanceof List) {
      List<?> list = (List<?>) value;
      if (list.isEmpty()) throw owner.unmeasured("the string form of an empty array in getDetail()");
      StringBuilder out = new StringBuilder("[ ");
      String separator = "";
      for (Object item : list) { out.append(separator).append(crest(item, owner)); separator = ", "; }
      return out.append(" ]").toString();
    }
    if (value instanceof String) {
      String text = (String) value;
      for (int i = 0; i < text.length(); i++) {
        char c = text.charAt(i);
        if (c < 0x20 || c > 0x7e || c == '"' || c == '\\') throw owner.unmeasured("the string form of a getDetail() string needing an escape");
      }
      return '"' + text + '"';
    }
    if (value instanceof Number) {
      double number = ((Number) value).doubleValue();
      if (number != Math.rint(number) || Math.abs(number) > 1e15) throw owner.unmeasured("the string form of a non-integral number in getDetail()");
      return Long.toString((long) number);
    }
    if (value == null || value instanceof Boolean) return String.valueOf(value);
    throw owner.unmeasured("the string form of a " + value.getClass().getSimpleName() + " in getDetail()");
  }

  private final class MapValue extends Value {
    private final Map<String, Object> map;
    private final BaseFunction toJson;
    private final String className;
    MapValue(Map<String, Object> map) {
      this(map, "ScriptableMap");
    }
    MapValue(Map<String, Object> map, String className) {
      super(false);
      this.map = map;
      this.className = className;
      toJson = new BaseFunction(scope, ScriptableObject.getFunctionPrototype(scope)) {
        @Override public Object call(Context cx, Scriptable s, Scriptable self, Object[] args) {
          return nativeValue(snapshot(map));
        }
      };
    }
    @Override Object backing() { return map; }
    @Override public String getClassName() { return className; }
    @Override public Object get(String name, Scriptable start) {
      Object value;
      synchronized (map) {
        if (!map.containsKey(name)) return "toJSON".equals(name) ? toJson : NOT_FOUND;
        value = map.get(name);
      }
      return wrap(value);
    }
    @Override public Object get(int index, Scriptable start) { return get(Integer.toString(index), start); }
    @Override public boolean has(String name, Scriptable start) { synchronized (map) { return map.containsKey(name) || "toJSON".equals(name); } }
    @Override public boolean has(int index, Scriptable start) { return has(Integer.toString(index), start); }
    @Override public void put(String name, Scriptable start, Object value) { Object converted = javaValue(value); synchronized (map) { map.put(name, converted); } }
    @Override public void put(int index, Scriptable start, Object value) { put(Integer.toString(index), start, value); }
    @Override public void delete(String name) { synchronized (map) { map.remove(name); } }
    @Override public void delete(int index) { delete(Integer.toString(index)); }
    @Override public Object[] getIds() { synchronized (map) { return map.keySet().toArray(); } }
  }

  private final class ListValue extends Value {
    private final List<Object> list;
    @SuppressWarnings("unchecked")
    ListValue(List<?> list) { super(true); this.list = (List<Object>) list; }
    @Override Object backing() { return list; }
    @Override public String getClassName() { return "Array"; }
    @Override public Object get(String name, Scriptable start) {
      if ("length".equals(name)) { synchronized (list) { return Integer.valueOf(list.size()); } }
      try { return get(Integer.parseInt(name), start); } catch (NumberFormatException e) { return NOT_FOUND; }
    }
    @Override public Object get(int index, Scriptable start) {
      Object value;
      synchronized (list) { if (index < 0 || index >= list.size()) return NOT_FOUND; value = list.get(index); }
      return wrap(value);
    }
    @Override public boolean has(String name, Scriptable start) {
      if ("length".equals(name)) return true;
      try { return has(Integer.parseInt(name), start); } catch (NumberFormatException e) { return false; }
    }
    @Override public boolean has(int index, Scriptable start) { synchronized (list) { return index >= 0 && index < list.size() && list.get(index) != null; } }
    @Override public void put(String name, Scriptable start, Object value) {
      if ("length".equals(name)) {
        int length = (int) Context.toNumber(value);
        if (length < 0) throw Context.reportRuntimeError("Invalid array length");
        synchronized (list) {
          while (list.size() > length) list.remove(list.size() - 1);
          while (list.size() < length) list.add(null);
        }
      } else {
        try { put(Integer.parseInt(name), start, value); } catch (NumberFormatException e) {
          throw Context.reportRuntimeError("rhino-local: non-index list property " + name);
        }
      }
    }
    @Override public void put(int index, Scriptable start, Object value) {
      if (index < 0) return;
      Object converted = javaValue(value);
      synchronized (list) {
        while (list.size() <= index) list.add(null);
        list.set(index, converted);
      }
    }
    @Override public void delete(String name) {
      try { delete(Integer.parseInt(name)); } catch (NumberFormatException e) { /* absent */ }
    }
    @Override public void delete(int index) { synchronized (list) { if (index >= 0 && index < list.size()) list.set(index, null); } }
    @Override public Object[] getIds() {
      synchronized (list) {
        Object[] ids = new Object[list.size()];
        for (int i = 0; i < ids.length; i++) ids[i] = Integer.valueOf(i);
        return ids;
      }
    }
  }

  static String pretty(Object value) {
    String json = Json.stringify(wireValue(value));
    StringBuilder out = new StringBuilder();
    int depth = 0;
    boolean quoted = false;
    boolean escaped = false;
    for (int i = 0; i < json.length(); i++) {
      char c = json.charAt(i);
      if (quoted) {
        out.append(c);
        if (escaped) escaped = false;
        else if (c == '\\') escaped = true;
        else if (c == '"') quoted = false;
      } else if (c == '"') { quoted = true; out.append(c); }
      else if (c == '{' || c == '[') {
        out.append(c);
        depth++;
        if (i + 1 < json.length() && json.charAt(i + 1) != '}' && json.charAt(i + 1) != ']') newline(out, depth);
      } else if (c == '}' || c == ']') {
        depth--;
        if (i > 0 && json.charAt(i - 1) != '{' && json.charAt(i - 1) != '[') newline(out, depth);
        out.append(c);
      } else if (c == ',') { out.append(c); newline(out, depth); }
      else if (c == ':') out.append(" : ");
      else out.append(c);
    }
    return out.toString();
  }

  private static void newline(StringBuilder out, int depth) {
    out.append('\n');
    for (int n = 0; n < depth; n++) out.append("  ");
  }
}
