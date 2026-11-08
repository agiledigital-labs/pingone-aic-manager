import java.util.Arrays;
import java.util.HashSet;
import java.util.Set;
import org.mozilla.javascript.NativeJavaObject;
import org.mozilla.javascript.Scriptable;

/**
 * {@code e.javaException} for a tenant error caught in an IDM script. On AIC it is a CREST
 * ResourceException subclass; here it is {@link HostCalls.HostException}, which is not, so every
 * member that would expose the difference faults {@code idm-exception/unmeasured} instead of
 * answering for the wrong class. Measured (probe-def and probe-pol, 2026-11-08, docs/api/10):
 * {@code getCode()}, {@code getReason()} and {@code String(getDetail())}.
 *
 * <p>{@code getClass} faults on read (sol-e15ab r6). AIC's {@code getClass().getName()} is measured,
 * but a hand-built stand-in for the method and the class it returns could not be made
 * indistinguishable from Rhino's native Java method and class: function names, source text,
 * receiver and arity checks, identity across exceptions, the Java string the name returns, and
 * engine diagnostics that read the implementation class directly all differed, across three review
 * rounds. The CREST class is in {@code e.message} / {@code String(e)}, which are measured.
 *
 * <p>Membership and enumeration follow the same gate as a read (sol-r3 P2-2): {@code name in view}
 * is {@code true} for a measured member and for {@code getClass} (the member exists on AIC's object
 * too), and faults for every other name, because answering {@code false} (or {@code true}) would
 * be the local class's answer. Enumeration ({@code Object.keys}, {@code for…in},
 * {@code JSON.stringify}) always faults: no probe listed AIC's members, and the local list is
 * HostException's. {@code Object.getOwnPropertyNames} and {@code Object.getOwnPropertyDescriptor}
 * are refused by Rhino with a TypeError for every Java object before it asks the object. Symbol-keyed
 * operations are left as Rhino answers them for any Java object.
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
    if ("getClass".equals(name)) {
      throw exception.unmeasured("e.javaException.getClass (AIC's getClass().getName() is measured, but this emulator cannot reproduce the Java method and class faithfully; the CREST class is in e.message and String(e))");
    }
    throw exception.unmeasured("e.javaException." + name + " (only getCode, getDetail, getReason and toString are measured)");
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
}
