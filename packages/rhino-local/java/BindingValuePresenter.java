import java.util.List;
import java.util.Map;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.NativeJavaList;
import org.mozilla.javascript.NativeJavaMap;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.WrapFactory;

/**
 * How a Java value reaches a script. A Java string, number or boolean arrives as the JavaScript
 * primitive (measured: {@code typeof} a list element is {@code "string"}); a {@code List} is a
 * {@code [object JavaList]} over a {@link SequenceFacade}; a {@code Map} is a {@code [object
 * JavaMap]} over a {@link DictionaryFacade}; anything else is Rhino's plain Java object.
 */
final class BindingValuePresenter extends WrapFactory {
  BindingValuePresenter() {
    setJavaPrimitiveWrap(false);
  }

  @Override
  public Scriptable wrapAsJavaObject(Context cx, Scriptable scope, Object javaObject, Class<?> staticType) {
    if (javaObject instanceof SequenceFacade) {
      return new NativeJavaList(scope, javaObject);
    }
    if (javaObject instanceof DictionaryFacade) {
      return new NativeJavaMap(scope, javaObject);
    }
    if (javaObject instanceof List) {
      return new NativeJavaList(scope, new SequenceFacade((List<?>) javaObject));
    }
    if (javaObject instanceof Map) {
      return new NativeJavaMap(scope, new DictionaryFacade((Map<?, ?>) javaObject));
    }
    return super.wrapAsJavaObject(cx, scope, javaObject, staticType);
  }
}
