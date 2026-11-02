import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.Map;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.WrappedException;

/** Opt-in AM bridge. Reuses IDM's host transport and sticky fatal state, not its evaluator. */
final class AmHostBridge extends BaseFunction {
  final IdmContextFactory state;
  final HostCalls calls;
  AmHostBridge(String id, long timeoutMs) {
    state = new IdmContextFactory(timeoutMs, "AM harness timeout");
    calls = new HostCalls(id, state);
  }
  @Override
  public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
    String method = Context.toString(args[0]);
    if ("missing".equals(method)) {
      String behavior = Context.toString(args[1]);
      EmulationFailure failure = new EmulationFailure("missing", "Missing journey behavior: " + behavior);
      failure.info.put("missing", java.util.Collections.singletonList(behavior));
      throw state.fail(failure);
    }
    Object input = Json.parse(Context.toString(args[1]));
    Map<String, Object> envelope = new LinkedHashMap<String, Object>();
    try { envelope.put("result", calls.call(method, input)); }
    catch (WrappedException e) {
      if (!(e.getWrappedException() instanceof HostCalls.HostException)) throw e;
      HostCalls.HostException host = (HostCalls.HostException) e.getWrappedException();
      Map<String, Object> error = new LinkedHashMap<String, Object>();
      error.put("code", String.valueOf(host.getCode())); error.put("message", host.getMessage());
      error.put("detail", host.detail()); error.put("class", host.exceptionClass());
      envelope.put("error", error);
    }
    return Json.stringify(envelope);
  }
  Map<String, Object> finish(String id, Map<String, Object> response) {
    calls.close();
    if (state.failure != null) {
      response = Runner.errorResponse(id, "timeout".equals(state.failure.info.get("kind")) ? "timeout" : "runtime_error", state.failure, null, -1, -1, null);
      Map<String, Object> fatal = new LinkedHashMap<String, Object>(state.failure.info);
      if (!state.missing.isEmpty()) fatal.put("missing", new ArrayList<String>(state.missing));
      response.put("emulation", fatal);
    }
    response.put("missing", new ArrayList<String>(state.missing));
    response.put("provenance", calls.evidenceSnapshot());
    if (!calls.unanswered.isEmpty()) response.put("unanswered", new ArrayList<Integer>(calls.unanswered));
    return response;
  }
}
