import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.Undefined;

/**
 * One script-tester job's harness faults: refusals of behaviour the harness does not emulate,
 * which must fail the run even when the script catches them (E19).
 *
 * <p>Defined for the preamble only, like {@code __rhinoLocalHostOp}; the runtime captures it at
 * seed time. One instance per job and never static, so a pooled JVM cannot carry a fault from one
 * case into another. The runner attaches what it holds to the job's response whatever the outcome
 * — a script that catches a fault and then throws, or runs out of time, still reports it.
 */
final class HarnessFaults extends BaseFunction {
  static final String NAME = "__rhinoLocalHarnessFault";
  private final List<Map<String, Object>> faults = new ArrayList<Map<String, Object>>();

  @Override
  public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
    Map<String, Object> fault = new LinkedHashMap<String, Object>();
    fault.put("id", Context.toString(args.length > 0 ? args[0] : Undefined.instance));
    fault.put("message", Context.toString(args.length > 1 ? args[1] : Undefined.instance));
    synchronized (faults) {
      faults.add(fault);
    }
    return Undefined.instance;
  }

  /** Adds {@code emulation} to the response when this job recorded any fault. */
  Map<String, Object> attach(Map<String, Object> response) {
    List<Map<String, Object>> held;
    synchronized (faults) {
      held = new ArrayList<Map<String, Object>>(faults);
    }
    if (held.isEmpty()) return response;
    List<String> causes = new ArrayList<String>();
    for (Map<String, Object> fault : held) {
      String cause = fault.get("id") + ": " + strip((String) fault.get("message"));
      if (!causes.contains(cause)) causes.add(cause);
    }
    Map<String, Object> info = new LinkedHashMap<String, Object>();
    info.put("kind", "missing");
    info.put(
        "message",
        "rhino-local harness fault " + causes.get(0)
            + " (not emulated: this fails the run even if the script caught it)");
    info.put("missing", causes);
    info.put("faults", held);
    response.put("emulation", info);
    return response;
  }

  private static String strip(String message) {
    return message.startsWith("rhino-local: ") ? message.substring(13) : message;
  }
}
