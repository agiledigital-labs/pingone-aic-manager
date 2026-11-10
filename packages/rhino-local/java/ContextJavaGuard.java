import java.util.Map;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Callable;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Function;
import org.mozilla.javascript.JavaScriptException;
import org.mozilla.javascript.NativeJavaMap;
import org.mozilla.javascript.NativeJavaObject;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Wrapper;

/**
 * Real Java objects that a legacy script context hands over (a {@code java.lang.String} from a
 * getter, the OIDC {@code claims} {@code LinkedHashMap}), with only their measured members
 * admitted (Sol ctx r2 P2-6).
 *
 * <p>A real Java object is what makes the measured comparisons hold: {@code a == b} between two
 * getter results is true on AIC, and only two Rhino {@link Wrapper}s over equal strings compare
 * that way. But a bare wrapper also exposes the whole native surface, so {@code substring},
 * {@code clear()} or a wrong-count call would answer as this JVM does, where no capture measured
 * AIC. These wrappers keep the {@link Wrapper} identity (so equality, {@code instanceof}, String
 * conversion, the {@code [object JavaObject]} tag and passing the object to a Java method all stay
 * native) and gate member lookup: an admitted member answers at its measured argument counts, a
 * Java or prototype member that is not admitted is a named harness fault, and a name that is
 * neither stays Rhino's own {@code undefined}.
 *
 * <p>Results of admitted methods that are Java strings come back guarded too, so
 * {@code claims.get("sub").substring(0, 1)} faults as {@code identity.getName().substring} does.
 *
 * <p>Known limit, as for {@link IdmExceptionView}: a native method borrowed from another object,
 * or the object passed as an argument to Java code, reaches the backing object unguarded.
 */
final class ContextJavaGuard {
  private ContextJavaGuard() {}

  /** {@code fault(member, detail)} is the context's {@code __rhinoLocalCtxUnmeasured}. */
  static RuntimeException fault(Context cx, Scriptable scope, Function fault, String member, String detail) {
    Object err = fault.call(cx, scope, scope, new Object[] {member, detail == null ? "" : detail});
    return new JavaScriptException(err, null, 0);
  }

  /** Measured argument counts per member, from a JS object {name: count | [counts]}. */
  static int[] arities(Scriptable admitted, String name) {
    if (admitted == null || !ScriptableObject.hasProperty(admitted, name)) return null;
    Object v = ScriptableObject.getProperty(admitted, name);
    if (v instanceof Scriptable && ScriptableObject.hasProperty((Scriptable) v, "length")) {
      Scriptable arr = (Scriptable) v;
      int n = (int) Context.toNumber(ScriptableObject.getProperty(arr, "length"));
      int[] out = new int[n];
      for (int i = 0; i < n; i++) out[i] = (int) Context.toNumber(ScriptableObject.getProperty(arr, i));
      return out;
    }
    return new int[] {(int) Context.toNumber(v)};
  }

  static Object guardResult(Context cx, Scriptable scope, Function fault, String label, Object value) {
    Object raw = value instanceof Wrapper ? ((Wrapper) value).unwrap() : null;
    if (raw instanceof String && !(value instanceof GuardedString)) {
      return new GuardedString(scope, (String) raw, label, fault);
    }
    return value;
  }

  /** An admitted method, callable only at its measured argument counts. */
  static final class Gate extends BaseFunction {
    private static final long serialVersionUID = 1L;
    private final transient Callable target;
    private final transient Scriptable self;
    private final int[] counts;
    private final String label;
    private final String name;
    private final transient Function fault;

    Gate(Scriptable scope, Callable target, Scriptable self, int[] counts, String label, String name, Function fault) {
      this.target = target;
      this.self = self;
      this.counts = counts;
      this.label = label;
      this.name = name;
      this.fault = fault;
      setParentScope(scope);
      setPrototype(ScriptableObject.getFunctionPrototype(scope));
    }

    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      boolean ok = false;
      StringBuilder measured = new StringBuilder();
      for (int c : counts) {
        if (c == args.length) ok = true;
        if (measured.length() > 0) measured.append(" or ");
        measured.append(c);
      }
      if (!ok) {
        throw fault(cx, scope, fault, label + "." + name + "(" + args.length + " argument(s))",
            "with " + args.length + " argument(s); only " + measured + " was measured");
      }
      return guardResult(cx, scope, fault, "java.lang.String", target.call(cx, scope, self, args));
    }

    @Override
    public String getFunctionName() {
      return name;
    }
  }

  /** Shared member gate: admitted → Gate, other existing member → fault, nothing → NOT_FOUND. */
  static Object member(Context cx, Scriptable scope, Scriptable self, Object found, Scriptable admitted,
      String label, String name, Function fault) {
    if (found == Scriptable.NOT_FOUND) return found;
    int[] counts = arities(admitted, name);
    if (counts == null) {
      throw fault(cx, scope, fault, label + "." + name, "(a member of the real Java object no capture measured)");
    }
    if (found instanceof Callable) return new Gate(scope, (Callable) found, self, counts, label, name, fault);
    return found;
  }

  static Object lookup(NativeJavaObject self, String name, Object own) {
    if (own != Scriptable.NOT_FOUND) return own;
    Scriptable proto = self.getPrototype();
    return proto == null ? Scriptable.NOT_FOUND : ScriptableObject.getProperty(proto, name);
  }

  /** A java.lang.String: measured {@code length} (a method) and {@code toString()}. */
  static final class GuardedString extends NativeJavaObject {
    private static final long serialVersionUID = 1L;
    private final transient Function fault;
    private final String label;
    private final transient Scriptable admitted;

    GuardedString(Scriptable scope, String text, String label, Function fault) {
      super(scope, text, String.class);
      this.fault = fault;
      this.label = label;
      Context cx = Context.getCurrentContext();
      Scriptable a = cx.newObject(scope);
      ScriptableObject.putProperty(a, "length", Integer.valueOf(0));
      ScriptableObject.putProperty(a, "toString", Integer.valueOf(0));
      this.admitted = a;
    }

    @Override
    public Object get(String name, Scriptable start) {
      Context cx = Context.getCurrentContext();
      return member(cx, getParentScope(), this, lookup(this, name, super.get(name, start)), admitted, label, name, fault);
    }

    @Override
    public Object get(int index, Scriptable start) {
      throw fault(Context.getCurrentContext(), getParentScope(), fault, label + "[" + index + "]", null);
    }

    @Override
    public boolean has(String name, Scriptable start) {
      if (arities(admitted, name) != null) return true;
      if (lookup(this, name, super.get(name, start)) == Scriptable.NOT_FOUND) return false;
      throw fault(Context.getCurrentContext(), getParentScope(), fault, "'" + name + "' in " + label, null);
    }

    @Override
    public void put(String name, Scriptable start, Object value) {
      throw fault(Context.getCurrentContext(), getParentScope(), fault, label + "." + name, "(assigning to a member of the Java object)");
    }

    @Override
    public Object[] getIds() {
      throw fault(Context.getCurrentContext(), getParentScope(), fault, "enumerating " + label, null);
    }
  }

  /**
   * A java.util.Map with the context factory's map access: an entry reads as a member (its string
   * value guarded), a name that is neither an entry nor a Java member is {@code undefined}, and
   * only the admitted methods answer. Assigning a member is unmeasured.
   */
  static final class GuardedMap extends NativeJavaMap {
    private static final long serialVersionUID = 1L;
    private final transient Map<?, ?> map;
    private final transient Scriptable admitted;
    private final String label;
    private final transient Function fault;

    GuardedMap(Scriptable scope, Map<?, ?> map, Scriptable admitted, String label, Function fault) {
      super(scope, map);
      this.map = map;
      this.admitted = admitted;
      this.label = label;
      this.fault = fault;
    }

    @Override
    public Object get(String name, Scriptable start) {
      Context cx = Context.getCurrentContext();
      Object found = super.get(name, start);
      if (map.containsKey(name)) return guardResult(cx, getParentScope(), fault, "java.lang.String", found);
      return member(cx, getParentScope(), this, lookup(this, name, found), admitted, label, name, fault);
    }

    @Override
    public void put(String name, Scriptable start, Object value) {
      throw fault(Context.getCurrentContext(), getParentScope(), fault, label + "." + name, "(assigning a member of the Java map)");
    }

    @Override
    public void put(int index, Scriptable start, Object value) {
      throw fault(Context.getCurrentContext(), getParentScope(), fault, label + "[" + index + "]", "(assigning a member of the Java map)");
    }
  }

  /** The {@code __rhinoLocalHostOp} entry points. */
  static Object dispatch(Context cx, Scriptable scope, String op, Object[] args) {
    switch (op) {
      case "guardString":
        // A fresh String object, as a getter's own result is: two of them are
        // == but not === (measured for claims.sub against identity.getName()).
        return new GuardedString(scope, new String(Context.toString(args[1])), Context.toString(args[2]), (Function) args[3]);
      case "guardMap": {
        Object raw = args[1] instanceof Wrapper ? ((Wrapper) args[1]).unwrap() : args[1];
        return new GuardedMap(scope, (Map<?, ?>) raw, (Scriptable) args[2], Context.toString(args[3]), (Function) args[4]);
      }
      case "unguard": {
        Object v = args[1];
        if (v instanceof GuardedString || v instanceof GuardedMap) {
          return cx.getWrapFactory().wrap(cx, scope, ((Wrapper) v).unwrap(), null);
        }
        return v;
      }
      default:
        return null;
    }
  }
}
