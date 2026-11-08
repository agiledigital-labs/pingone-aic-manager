import java.util.Arrays;
import java.util.HashSet;
import java.util.Set;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.NativeJavaObject;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Symbol;
import org.mozilla.javascript.SymbolScriptable;
import org.mozilla.javascript.Wrapper;

/**
 * {@code e.javaException} for a tenant error caught in an IDM script. On AIC it is a CREST
 * ResourceException subclass; here it is {@link HostCalls.HostException}, which is not, so every
 * member that would expose the difference faults {@code idm-exception/unmeasured} instead of
 * answering for the wrong class. Measured (probe-def and probe-pol, 2026-11-08, docs/api/10):
 * {@code getCode()}, {@code getReason()}, {@code String(getDetail())} and
 * {@code getClass().getName()}, the last equal to the class the message names. Reproducing the
 * class itself would mean shipping the vendor class, so {@code getClass()} answers only
 * {@code getName()}.
 *
 * <p>Membership and enumeration follow the same gate as a read (sol-r3 P2-2): {@code name in view}
 * is {@code true} for a name a read admits, because that member exists on AIC's object too, and
 * faults for every other name, because answering {@code false} (or {@code true}) would be the local
 * class's answer. Enumeration ({@code Object.keys}, {@code for…in}, {@code JSON.stringify},
 * {@code Object.getOwnPropertyNames}) always faults: no probe listed AIC's members, and the local
 * list is HostException's, except {@code Object.getOwnPropertyNames} and
 * {@code Object.getOwnPropertyDescriptor}, which Rhino refuses with a TypeError for every Java
 * object before asking it. The {@code getClass()} shim does the same with {@code getName}.
 * Symbol-keyed operations on the view are left as Rhino answers them for any Java object; on the
 * {@code getClass()} shim they fault, because the shim is not a Java object.
 */
final class IdmExceptionView extends NativeJavaObject {
  private static final long serialVersionUID = 1L;
  /** Members passed to the real object. Rhino's own String conversion calls Object.toString directly. */
  private static final Set<String> ADMITTED = new HashSet<String>(Arrays.asList("getCode", "getDetail", "getReason", "toString"));
  private final transient HostCalls.HostException exception;
  private transient ClassFunction classFunction;

  IdmExceptionView(Scriptable scope, HostCalls.HostException exception) {
    super(scope, exception, HostCalls.HostException.class);
    this.exception = exception;
  }

  @Override
  public Object get(String name, Scriptable start) {
    if (ADMITTED.contains(name)) return super.get(name, start);
    // One function per view, as Rhino keeps one NativeJavaMethod per member: getClass === getClass.
    if ("getClass".equals(name)) { if (classFunction == null) classFunction = new ClassFunction(getParentScope()); return classFunction; }
    throw exception.unmeasured("e.javaException." + name + " (only getCode, getDetail, getReason, toString and getClass().getName() are measured)");
  }

  @Override
  public Object get(int index, Scriptable start) {
    throw exception.unmeasured("e.javaException[" + index + "]");
  }

  @Override
  public boolean has(String name, Scriptable start) {
    if (ADMITTED.contains(name) || "getClass".equals(name)) return true;
    // for…in asks for an __iterator__ before it enumerates.
    if ("__iterator__".equals(name)) throw exception.unmeasured("enumerating e.javaException's members (no probe listed them)");
    throw exception.unmeasured("'" + name + "' in e.javaException (only getCode, getDetail, getReason, toString and getClass are measured members)");
  }

  @Override
  public boolean has(int index, Scriptable start) {
    throw exception.unmeasured(index + " in e.javaException");
  }

  @Override
  public Object[] getIds() {
    throw exception.unmeasured("enumerating e.javaException's members (no probe listed them)");
  }

  /** {@code getClass()}: an object answering only {@code getName()}. */
  private final class ClassFunction extends BaseFunction {
    private static final long serialVersionUID = 1L;
    ClassFunction(Scriptable scope) { super(scope, ScriptableObject.getFunctionPrototype(scope)); }
    @Override public Object call(Context cx, Scriptable scope, Scriptable self, Object[] args) {
      return new ClassView(getParentScope());
    }
  }

  /**
   * The {@code getClass()} result. A plain {@link Scriptable}, not a {@link ScriptableObject}, so
   * that Rhino treats it as it treats any Java object (AIC's real {@code Class} is one): the
   * {@code Object.getOwnPropertyNames} / {@code getOwnPropertyDescriptor} family refuses it with a
   * TypeError before asking it, as it does the exception view, instead of answering an empty list.
   * Every other operation but {@code getName} faults, including the two a plain {@link Scriptable}
   * would otherwise answer for itself (sol-e15ab r4 P2-1): its class tag, which a borrowed
   * {@code Object.prototype.toString} reads without going through {@code get}, and symbol-keyed
   * access, which Rhino refuses with a catchable TypeError for a non-{@link SymbolScriptable}.
   * It is a {@link Wrapper} whose {@code unwrap} faults (sol-e15ab r5 P2-1): a Java
   * {@code instanceof} with the shim on the left ({@code c instanceof java.lang.Class}) and strict
   * equality between two {@code getClass()} results both unwrap their operands, and otherwise
   * answered from the shim's own type and fresh identity (false, where a real Java class is true).
   */
  private final class ClassView implements Scriptable, SymbolScriptable, Wrapper {
    private Scriptable parent;
    ClassView(Scriptable scope) { parent = scope; }
    private EmulationFailure unmeasured(String what) {
      return exception.unmeasured(what + " on e.javaException.getClass() (only getName() is measured)");
    }
    @Override public Object unwrap() { throw unmeasured("Java identity or type (instanceof a Java class, or comparing two getClass() results)"); }
    @Override public String getClassName() { throw unmeasured("the class tag (Object.prototype.toString)"); }
    @Override public Object get(Symbol key, Scriptable start) { throw unmeasured("reading a symbol-keyed member"); }
    @Override public boolean has(Symbol key, Scriptable start) { throw unmeasured("a symbol-keyed `in`"); }
    @Override public void put(Symbol key, Scriptable start, Object value) { throw unmeasured("writing a symbol-keyed member"); }
    @Override public void delete(Symbol key) { throw unmeasured("deleting a symbol-keyed member"); }
    @Override public Object get(String name, Scriptable start) {
      if ("getName".equals(name)) {
        return new BaseFunction(parent, ScriptableObject.getFunctionPrototype(parent)) {
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
    @Override public boolean has(String name, Scriptable start) {
      if ("getName".equals(name)) return true;
      if ("__iterator__".equals(name)) throw unmeasured("enumerating the members");
      throw exception.unmeasured("'" + name + "' in e.javaException.getClass() (only getName is a measured member)");
    }
    @Override public boolean has(int index, Scriptable start) { throw unmeasured(index + " in"); }
    @Override public void put(String name, Scriptable start, Object value) { throw unmeasured("writing " + name); }
    @Override public void put(int index, Scriptable start, Object value) { throw unmeasured("writing [" + index + "]"); }
    @Override public void delete(String name) { throw unmeasured("deleting " + name); }
    @Override public void delete(int index) { throw unmeasured("deleting [" + index + "]"); }
    @Override public Scriptable getPrototype() { return null; }
    @Override public void setPrototype(Scriptable prototype) { throw unmeasured("setting the prototype"); }
    @Override public Scriptable getParentScope() { return parent; }
    @Override public void setParentScope(Scriptable scope) { parent = scope; }
    @Override public Object[] getIds() { throw unmeasured("enumerating the members"); }
    @Override public Object getDefaultValue(Class<?> hint) {
      throw exception.unmeasured("the string form of e.javaException.getClass()");
    }
    @Override public boolean hasInstance(Scriptable instance) { throw unmeasured("instanceof"); }
  }
}
