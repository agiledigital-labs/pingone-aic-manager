import java.util.ArrayList;
import java.util.List;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;
import org.mozilla.javascript.Wrapper;

/** Real Java collections for the bindings AIC hands over as Java collections. */
final class JavaCollections {
  private JavaCollections() {}

  /** HostOps "javaList": a script array's elements as a presented {@code java.util.ArrayList}. */
  static Object list(Context cx, Scriptable scope, Object array, boolean immutable) {
    List<Object> list = new ArrayList<Object>();
    if (array instanceof Scriptable) {
      Scriptable items = (Scriptable) array;
      long length = org.mozilla.javascript.ScriptRuntime.toUint32(ScriptableObject.getProperty(items, "length"));
      for (int i = 0; i < length; i += 1) {
        list.add(element(ScriptableObject.getProperty(items, i)));
      }
    } else if (array != null && array != Undefined.instance) {
      throw Context.reportRuntimeError("rhino-local: javaList needs an array");
    }
    // An immutable list refuses add() and the other writers as Java does: an
    // UnsupportedOperationException with no message.
    if (immutable) {
      list = java.util.Collections.unmodifiableList(list);
    }
    return cx.getWrapFactory().wrapAsJavaObject(cx, ScriptableObject.getTopLevelScope(scope), list, List.class);
  }

  /** What a script value is once it is inside a Java list. */
  private static Object element(Object value) {
    if (value == Scriptable.NOT_FOUND || value == Undefined.instance) {
      return null;
    }
    if (value instanceof CharSequence) {
      return value.toString();
    }
    if (value instanceof Wrapper) {
      return ((Wrapper) value).unwrap();
    }
    return value;
  }

  /** HostOps "javaListItems". */
  static Object items(Context cx, Scriptable scope, Object value) {
    Object raw = value instanceof Wrapper ? ((Wrapper) value).unwrap() : null;
    if (!(raw instanceof SequenceFacade)) {
      return null;
    }
    List<Object> list = ((SequenceFacade) raw).delegate();
    Object[] items = new Object[list.size()];
    Scriptable top = ScriptableObject.getTopLevelScope(scope);
    for (int i = 0; i < items.length; i += 1) {
      Object item = list.get(i);
      items[i] = item == null ? null : Context.javaToJS(item, top);
    }
    return cx.newArray(top, items);
  }
}
