import org.mozilla.javascript.Context;
import org.mozilla.javascript.Function;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;

/**
 * A prototype that answers every integer-indexed read its object does not hold itself (Sol ctx r14
 * P2-1). Rhino 1.7.14 has no Proxy, so a JavaScript collection can trap only the indices it
 * defines; {@code typeof scopes[8]} on an empty view then read {@code undefined} past the eight
 * guarded slots. Rhino's {@link ScriptableObject#getProperty(Scriptable, int)} walks the prototype
 * chain, so this object sees every index the collection lacks and hands it to the collection's own
 * read decision, which either throws (the named fault, or AIC's measured error) or returns, in
 * which case the read is {@link Scriptable#NOT_FOUND} as before.
 *
 * <p>It holds no indices and reports none, so {@code in}, {@code for...in} and {@code
 * Object.keys} see nothing of it. Only reads go through it: a write lands on the collection, where
 * the harvest audit finds it.
 */
final class IndexTrap extends ScriptableObject {
  private static final long serialVersionUID = 1L;
  private final transient Function read;

  IndexTrap(Scriptable scope, Scriptable proto, Function read) {
    this.read = read;
    setParentScope(ScriptableObject.getTopLevelScope(scope));
    setPrototype(proto);
  }

  @Override
  public String getClassName() {
    return "Object";
  }

  @Override
  public Object get(int index, Scriptable start) {
    if (start != this) {
      Context cx = Context.getCurrentContext();
      read.call(cx, getParentScope(), start, new Object[] {Integer.valueOf(index)});
    }
    return NOT_FOUND;
  }

  /** HostOps "indexTrap": {@code (proto | null, read)}. */
  static Object create(Scriptable scope, Object[] args) {
    Scriptable proto = args.length > 1 && args[1] instanceof Scriptable ? (Scriptable) args[1] : null;
    if (args.length < 3 || !(args[2] instanceof Function)) {
      throw Context.reportRuntimeError("rhino-local: indexTrap needs a read function");
    }
    return new IndexTrap(scope, proto, (Function) args[2]);
  }
}
