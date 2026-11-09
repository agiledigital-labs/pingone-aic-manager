import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.Undefined;

/**
 * The runtime's bracket for phases only it can see the edges of: a library's
 * module body and a case's harvest ({@link Timing}).
 *
 * <p>Defined for the prelude only, and only on a job that asked for timing, so
 * the runtime captures {@code null} otherwise and a timed-off job pays one
 * null check per bracket. {@code ("begin", phase, label)} returns the depth;
 * {@code ("end", depth)} closes it.
 */
final class PhaseTimer extends BaseFunction {
  static final String NAME = "__rhinoLocalTimer";

  @Override
  public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
    String op = args.length > 0 ? Context.toString(args[0]) : "";
    if ("begin".equals(op)) {
      String phase = args.length > 1 ? Context.toString(args[1]) : "unknown";
      String label = args.length > 2 && args[2] instanceof CharSequence ? args[2].toString() : null;
      return Integer.valueOf(Timing.enter(phase, label));
    }
    if ("end".equals(op) && args.length > 1) {
      Timing.exit((int) Context.toNumber(args[1]));
    }
    return Undefined.instance;
  }
}
