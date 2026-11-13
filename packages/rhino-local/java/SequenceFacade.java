import java.util.Collection;
import java.util.Iterator;
import java.util.List;
import java.util.ListIterator;
import java.util.Map;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;

/**
 * The list a script is handed: a {@code java.util.List} that forwards every method to the real
 * list behind it. Rhino presents it as a {@code [object JavaList]}, so {@code length}, {@code
 * [i]}, {@code size()}, {@code JSON.stringify}, {@code [].concat(v)} and the rest are the
 * engine's own answers at every size, not ours.
 *
 * <p>The class shutter is asked about this class, never about the list behind it, which is why a
 * list binding works in a context whose allow-list has no {@code java.util.ArrayList}. What the
 * list's own methods return is still subject to the shutter: {@code iterator()} is the measured
 * {@code Access to Java class "java.util.ArrayList$Itr" is prohibited.}
 *
 * <p>{@code equals} and {@code hashCode} are identity.
 */
final class SequenceFacade implements List<Object> {
  private final List<Object> list;

  @SuppressWarnings("unchecked")
  SequenceFacade(List<?> list) {
    this.list = (List<Object>) list;
  }

  /** The list behind the facade, for the harness's own reads. */
  List<Object> delegate() {
    return list;
  }

  @Override public int size() { return list.size(); }
  @Override public boolean isEmpty() { return list.isEmpty(); }
  @Override public boolean contains(Object o) { return list.contains(o); }
  @Override public Iterator<Object> iterator() { return list.iterator(); }
  @Override public Object[] toArray() { return list.toArray(); }
  @Override public <T> T[] toArray(T[] a) { return list.toArray(a); }
  @Override public boolean add(Object o) { return list.add(o); }
  @Override public boolean remove(Object o) { return list.remove(o); }
  @Override public boolean containsAll(Collection<?> c) { return list.containsAll(c); }
  @Override public boolean addAll(Collection<?> c) { return list.addAll(c); }
  @Override public boolean addAll(int index, Collection<?> c) { return list.addAll(index, c); }
  @Override public boolean removeAll(Collection<?> c) { return list.removeAll(c); }
  @Override public boolean retainAll(Collection<?> c) { return list.retainAll(c); }
  @Override public void clear() { list.clear(); }
  @Override public Object get(int index) { return list.get(index); }
  @Override public Object set(int index, Object element) { return list.set(index, element); }
  @Override public void add(int index, Object element) { list.add(index, element); }
  @Override public Object remove(int index) { return list.remove(index); }
  @Override public int indexOf(Object o) { return list.indexOf(o); }
  @Override public int lastIndexOf(Object o) { return list.lastIndexOf(o); }
  @Override public ListIterator<Object> listIterator() { return list.listIterator(); }
  @Override public ListIterator<Object> listIterator(int index) { return list.listIterator(index); }
  @Override public List<Object> subList(int from, int to) { return list.subList(from, to); }

  /**
   * Java's own {@code [a, b]}. A record the emulator still holds as a script object is written
   * the way a Java map prints, {@code {k=v, k2=v2}}, which is the measured form of an openidm
   * list ({@code [{_id=…, _rev=…}]}).
   */
  @Override
  public String toString() {
    StringBuilder out = new StringBuilder();
    render(out, list, false);
    return out.toString();
  }

  /**
   * {@code record} is true inside a record the emulator holds as a script object, where every
   * number is a script number: a whole one prints as an integer field prints. A number that is
   * an element of a list itself prints as Java prints it, so one a script added reads {@code 5.0}.
   */
  private static void render(StringBuilder out, Object value, boolean record) {
    if (value instanceof SequenceFacade) {
      value = ((SequenceFacade) value).list;
    }
    if (value instanceof org.mozilla.javascript.Wrapper) {
      value = ((org.mozilla.javascript.Wrapper) value).unwrap();
      if (value instanceof SequenceFacade) {
        value = ((SequenceFacade) value).list;
      }
    }
    if (value instanceof org.mozilla.javascript.NativeArray || (value instanceof List && !(value instanceof Scriptable))) {
      out.append('[');
      boolean first = true;
      for (Object item : (List<?>) value) {
        if (!first) out.append(", ");
        first = false;
        render(out, item, record);
      }
      out.append(']');
    } else if (value instanceof ScriptableObject && "Object".equals(((ScriptableObject) value).getClassName())) {
      ScriptableObject object = (ScriptableObject) value;
      out.append('{');
      boolean first = true;
      for (Object id : object.getIds()) {
        if (!first) out.append(", ");
        first = false;
        out.append(id).append('=');
        render(out, id instanceof Integer
            ? ScriptableObject.getProperty(object, ((Integer) id).intValue())
            : ScriptableObject.getProperty(object, String.valueOf(id)), true);
      }
      out.append('}');
    } else if (value instanceof Map && !(value instanceof Scriptable)) {
      out.append('{');
      boolean first = true;
      for (Map.Entry<?, ?> entry : ((Map<?, ?>) value).entrySet()) {
        if (!first) out.append(", ");
        first = false;
        out.append(entry.getKey()).append('=');
        render(out, entry.getValue(), record);
      }
      out.append('}');
    } else if (value == null || value == Undefined.instance) {
      out.append("null");
    } else if (value instanceof Number) {
      double d = ((Number) value).doubleValue();
      if (record && value instanceof Double && d == Math.rint(d) && !Double.isInfinite(d) && Math.abs(d) < 1e15) {
        out.append((long) d);
      } else {
        out.append(value);
      }
    } else {
      out.append(value instanceof CharSequence ? value.toString() : String.valueOf(value));
    }
  }
}
