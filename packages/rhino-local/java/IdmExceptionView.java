import java.util.Arrays;
import java.util.HashSet;
import java.util.Set;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.NativeJavaObject;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;

/**
 * {@code e.javaException} for a tenant error caught in an IDM script. On AIC it is a CREST
 * ResourceException subclass; here it is {@link HostCalls.HostException}, which is not, so every
 * member that would expose the difference faults {@code idm-exception/unmeasured} instead of
 * answering for the wrong class. Measured (probe-def and probe-pol, 2026-11-08, docs/api/10):
 * {@code getCode()}, {@code getReason()}, {@code String(getDetail())} and
 * {@code getClass().getName()}, the last equal to the class the message names. Reproducing the
 * class itself would mean shipping the vendor class, so {@code getClass()} answers only
 * {@code getName()}.
 */
final class IdmExceptionView extends NativeJavaObject {
  private static final long serialVersionUID = 1L;
  /** Members passed to the real object. Rhino's own String conversion calls Object.toString directly. */
  private static final Set<String> ADMITTED = new HashSet<String>(Arrays.asList("getCode", "getDetail", "getReason", "toString"));
  private final transient HostCalls.HostException exception;

  IdmExceptionView(Scriptable scope, HostCalls.HostException exception) {
    super(scope, exception, HostCalls.HostException.class);
    this.exception = exception;
  }

  @Override
  public Object get(String name, Scriptable start) {
    if (ADMITTED.contains(name)) return super.get(name, start);
    if ("getClass".equals(name)) return new ClassFunction(getParentScope());
    throw exception.unmeasured("e.javaException." + name + " (only getCode, getDetail, getReason, toString and getClass().getName() are measured)");
  }

  @Override
  public Object get(int index, Scriptable start) {
    throw exception.unmeasured("e.javaException[" + index + "]");
  }

  /** {@code getClass()}: an object answering only {@code getName()}. */
  private final class ClassFunction extends BaseFunction {
    private static final long serialVersionUID = 1L;
    ClassFunction(Scriptable scope) { super(scope, ScriptableObject.getFunctionPrototype(scope)); }
    @Override public Object call(Context cx, Scriptable scope, Scriptable self, Object[] args) {
      return new ClassView(getParentScope());
    }
  }

  private final class ClassView extends ScriptableObject {
    private static final long serialVersionUID = 1L;
    ClassView(Scriptable scope) { setParentScope(scope); }
    @Override public String getClassName() { return "JavaClass"; }
    @Override public Object get(String name, Scriptable start) {
      if ("getName".equals(name)) {
        return new BaseFunction(getParentScope(), ScriptableObject.getFunctionPrototype(getParentScope())) {
          private static final long serialVersionUID = 1L;
          @Override public Object call(Context cx, Scriptable scope, Scriptable self, Object[] args) {
            return exception.exceptionClass();
          }
        };
      }
      throw exception.unmeasured("e.javaException.getClass()." + name + " (only getName() is measured)");
    }
    @Override public Object get(int index, Scriptable start) {
      throw exception.unmeasured("e.javaException.getClass()[" + index + "]");
    }
    @Override public Object getDefaultValue(Class<?> hint) {
      throw exception.unmeasured("the string form of e.javaException.getClass()");
    }
  }
}
