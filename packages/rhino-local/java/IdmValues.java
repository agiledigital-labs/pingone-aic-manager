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
  static final ThreadLocal<IdmValues> CURRENT = new ThreadLocal<IdmValues>();
  private final Scriptable scope;
  private final IdentityHashMap<Object, Scriptable> wrappers = new IdentityHashMap<Object, Scriptable>();

  IdmValues(Scriptable scope) { this.scope = scope; }

  static Object wrapCurrent(Object value) { return CURRENT.get().wrap(value); }

  Object wrap(Object value) {
    if (!(value instanceof Map) && !(value instanceof List)) return value;
    Scriptable cached = wrappers.get(value);
    if (cached != null) return cached;
    Value result = value instanceof Map ? new MapValue(Json.asObject(value, "map")) : new ListValue((List<?>) value);
    wrappers.put(value, result);
    return result;
  }

  Object wrapRequest(Object value) {
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
    return javaValue(value, new IdentityHashMap<Object, Boolean>());
  }

  private static Object javaValue(Object value, IdentityHashMap<Object, Boolean> active) {
    if (value == Scriptable.NOT_FOUND || Undefined.isUndefined(value)) return null;
    if (value instanceof Value) return ((Value) value).backing();
    if (value instanceof Wrapper) return javaValue(((Wrapper) value).unwrap(), active);
    if (value instanceof CharSequence) return value.toString();
    if (value instanceof Number) {
      double n = ((Number) value).doubleValue();
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
        for (int i = 0; i < length; i++) list.add(javaValue(scriptable.get(i, scriptable), active));
        return list;
      }
      Map<String, Object> map = new HashMap<String, Object>();
      for (Object id : scriptable.getIds()) {
        Object member = id instanceof Number ? scriptable.get(((Number) id).intValue(), scriptable)
            : scriptable.get(id.toString(), scriptable);
        map.put(id.toString(), javaValue(member, active));
      }
      return map;
    } finally {
      active.remove(value);
    }
  }

  /** Endpoint/protocol conversion must not alter numeric values already stored in maps. */
  static Object wireValue(Object value) {
    value = javaValue(value);
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
    private Scriptable prototype;
    private Scriptable parent = scope;
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
    @Override public Object getDefaultValue(Class<?> hint) { return pretty(backing()); }
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
          return nativeValue(map);
        }
      };
    }
    @Override Object backing() { return map; }
    @Override public String getClassName() { return className; }
    @Override public Object get(String name, Scriptable start) {
      if (map.containsKey(name)) return wrap(map.get(name));
      return "toJSON".equals(name) ? toJson : NOT_FOUND;
    }
    @Override public Object get(int index, Scriptable start) { return get(Integer.toString(index), start); }
    @Override public boolean has(String name, Scriptable start) { return map.containsKey(name) || "toJSON".equals(name); }
    @Override public boolean has(int index, Scriptable start) { return has(Integer.toString(index), start); }
    @Override public void put(String name, Scriptable start, Object value) { map.put(name, javaValue(value)); }
    @Override public void put(int index, Scriptable start, Object value) { put(Integer.toString(index), start, value); }
    @Override public void delete(String name) { map.remove(name); }
    @Override public void delete(int index) { delete(Integer.toString(index)); }
    @Override public Object[] getIds() { return map.keySet().toArray(); }
  }

  private final class ListValue extends Value {
    private final List<Object> list;
    @SuppressWarnings("unchecked")
    ListValue(List<?> list) { super(true); this.list = (List<Object>) list; }
    @Override Object backing() { return list; }
    @Override public String getClassName() { return "Array"; }
    @Override public Object get(String name, Scriptable start) {
      if ("length".equals(name)) return Integer.valueOf(list.size());
      try { return get(Integer.parseInt(name), start); } catch (NumberFormatException e) { return NOT_FOUND; }
    }
    @Override public Object get(int index, Scriptable start) {
      return index >= 0 && index < list.size() ? wrap(list.get(index)) : NOT_FOUND;
    }
    @Override public boolean has(String name, Scriptable start) {
      if ("length".equals(name)) return true;
      try { return has(Integer.parseInt(name), start); } catch (NumberFormatException e) { return false; }
    }
    @Override public boolean has(int index, Scriptable start) { return index >= 0 && index < list.size() && list.get(index) != null; }
    @Override public void put(String name, Scriptable start, Object value) {
      if ("length".equals(name)) {
        int length = (int) Context.toNumber(value);
        if (length < 0) throw Context.reportRuntimeError("Invalid array length");
        while (list.size() > length) list.remove(list.size() - 1);
        while (list.size() < length) list.add(null);
      } else {
        try { put(Integer.parseInt(name), start, value); } catch (NumberFormatException e) {
          throw Context.reportRuntimeError("rhino-local: non-index list property " + name);
        }
      }
    }
    @Override public void put(int index, Scriptable start, Object value) {
      if (index < 0) return;
      while (list.size() <= index) list.add(null);
      list.set(index, javaValue(value));
    }
    @Override public void delete(String name) {
      try { delete(Integer.parseInt(name)); } catch (NumberFormatException e) { /* absent */ }
    }
    @Override public void delete(int index) { if (index >= 0 && index < list.size()) list.set(index, null); }
    @Override public Object[] getIds() {
      Object[] ids = new Object[list.size()];
      for (int i = 0; i < ids.length; i++) ids[i] = Integer.valueOf(i);
      return ids;
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
