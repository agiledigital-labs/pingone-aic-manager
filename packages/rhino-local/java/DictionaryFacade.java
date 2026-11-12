import java.util.ArrayList;
import java.util.Collection;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * The map a script is handed: a {@code java.util.Map} over the real map behind it, which Rhino
 * presents as a {@code [object JavaMap]}. Most methods forward; {@code values()} and {@code
 * entrySet()} are copies, {@code keySet()} is live, and {@code keys()} and {@code entries()} are
 * list copies. {@code String(map)} is the measured {@code { "k": v }} form ({@code {  }} when
 * empty), not Java's {@code {k=v}}. {@code equals} and {@code hashCode} are identity.
 */
public final class DictionaryFacade implements Map<String, Object> {
  private final Map<String, Object> map;

  @SuppressWarnings("unchecked")
  DictionaryFacade(Map<?, ?> map) {
    this.map = (Map<String, Object>) map;
  }

  Map<String, Object> delegate() {
    return map;
  }

  @Override public int size() { return map.size(); }
  @Override public boolean isEmpty() { return map.isEmpty(); }
  @Override public boolean containsKey(Object key) { return map.containsKey(key); }
  @Override public boolean containsValue(Object value) { return map.containsValue(value); }
  @Override public Object get(Object key) { return map.get(key); }
  @Override public Object put(String key, Object value) { return map.put(key, value); }
  @Override public Object remove(Object key) { return map.remove(key); }
  @Override public void putAll(Map<? extends String, ?> m) { map.putAll(m); }
  @Override public void clear() { map.clear(); }
  @Override public Set<String> keySet() { return map.keySet(); }
  public List<String> keys() { return new ArrayList<String>(map.keySet()); }
  @Override public Collection<Object> values() { return new ArrayList<Object>(map.values()); }
  @Override public Set<Map.Entry<String, Object>> entrySet() { return new HashSet<Map.Entry<String, Object>>(map.entrySet()); }
  public List<Map.Entry<String, Object>> entries() { return new ArrayList<Map.Entry<String, Object>>(map.entrySet()); }

  @Override
  public String toString() {
    StringBuilder out = new StringBuilder();
    render(out, map);
    return out.toString();
  }

  private static void render(StringBuilder out, Object value) {
    if (value instanceof DictionaryFacade) value = ((DictionaryFacade) value).map;
    if (value instanceof SequenceFacade) value = ((SequenceFacade) value).delegate();
    if (value == null) {
      out.append("null");
    } else if (value instanceof Map) {
      out.append("{ ");
      Iterator<? extends Map.Entry<?, ?>> it = ((Map<?, ?>) value).entrySet().iterator();
      while (it.hasNext()) {
        Map.Entry<?, ?> entry = it.next();
        out.append('"').append(entry.getKey()).append("\": ");
        render(out, entry.getValue());
        if (it.hasNext()) out.append(", ");
      }
      out.append(" }");
    } else if (value instanceof Collection) {
      out.append("[ ");
      Iterator<?> it = ((Collection<?>) value).iterator();
      while (it.hasNext()) {
        render(out, it.next());
        if (it.hasNext()) out.append(", ");
      }
      out.append(" ]");
    } else if (value instanceof String) {
      out.append('"');
      String s = (String) value;
      for (int i = 0; i < s.length(); i += 1) {
        char c = s.charAt(i);
        switch (c) {
          case '"': out.append("\\\""); break;
          case '\\': out.append("\\\\"); break;
          case '\b': out.append("\\b"); break;
          case '\f': out.append("\\f"); break;
          case '\n': out.append("\\n"); break;
          case '\r': out.append("\\r"); break;
          case '\t': out.append("\\t"); break;
          default:
            if (Character.isISOControl(c)) out.append(String.format("\\u%04X", (int) c));
            else out.append(c);
        }
      }
      out.append('"');
    } else {
      out.append(value);
    }
  }
}
