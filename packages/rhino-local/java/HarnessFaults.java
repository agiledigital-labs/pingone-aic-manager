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
  /** A script that recurses past the stack. A script error, not an emulator failure. */
  static final String STACK_OVERFLOW_ID = "script-defect/stack-overflow";
  static final String STACK_OVERFLOW_MESSAGE = "the script exceeded the maximum stack depth";
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

  /**
   * Rhino's interpreter reports {@code Exceeded maximum stack depth}. A compiled
   * script throws {@link StackOverflowError}. Either may be wrapped.
   */
  static boolean isStackOverflow(Throwable error) {
    Throwable cursor = error;
    for (int depth = 0; cursor != null && depth < 8; depth++) {
      if (cursor instanceof StackOverflowError) return true;
      String message = cursor.getMessage();
      if (message != null && message.contains("Exceeded maximum stack depth")) return true;
      Throwable next = cursor.getCause();
      if (next == cursor) return false;
      cursor = next;
    }
    return false;
  }

  /** The emulation object {@link #attach} would add for this one fault. */
  static Map<String, Object> stackOverflowEmulation() {
    HarnessFaults recorded = new HarnessFaults();
    recorded.record(STACK_OVERFLOW_ID, STACK_OVERFLOW_MESSAGE);
    Map<String, Object> response = new LinkedHashMap<String, Object>();
    recorded.attach(response);
    return Json.asObject(response.get("emulation"), "stack overflow fault");
  }

  /** Records a fault from Java, as the preamble's {@code __rhinoLocalHarnessFault} call does. */
  void record(String id, String message) {
    Map<String, Object> fault = new LinkedHashMap<String, Object>();
    fault.put("id", id);
    fault.put("message", message);
    synchronized (faults) {
      faults.add(fault);
    }
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
    // The wording answers for every fault, not only the first one named
    // (round-3 P3-5): a case can hit a gap and a script defect together.
    int defects = 0;
    for (String cause : causes) {
      if (cause.startsWith("script-defect/")) defects += 1;
    }
    int gaps = causes.size() - defects;
    String why;
    if (gaps == 0) {
      why = " (script defect: AIC's behaviour here is measured and is not what the script means;"
          + " this fails the run even if the script caught it)";
    } else if (defects == 0) {
      why = " (not emulated: this fails the run even if the script caught it)";
    } else {
      why = " (and " + (causes.size() - 1) + " more: " + defects + " script defect(s), where AIC's"
          + " behaviour is measured and is not what the script means, and " + gaps
          + " behaviour(s) the harness does not emulate; this fails the run even if the script caught it)";
    }
    info.put("message", "rhino-local harness fault " + causes.get(0) + why);
    info.put("missing", causes);
    info.put("faults", held);
    response.put("emulation", info);
    return response;
  }

  private static String strip(String message) {
    return message.startsWith("rhino-local: ") ? message.substring(13) : message;
  }
}
