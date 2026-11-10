import org.mozilla.javascript.Context;
import org.mozilla.javascript.ContextFactory;
import org.mozilla.javascript.EvaluatorException;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.WrapFactory;

/**
 * Refuses an engine capability object reaching a script, by whatever route.
 *
 * <p>Measured 2026-11-10 (docs/api/12): AIC hides the engine package from a
 * script that names it, so a script cannot construct a {@code Context}. That
 * is a refusal of a <em>name</em>. A Java API that builds or returns the
 * instance for the script ({@code java.beans.XMLDecoder} on a legacy case is
 * one) gets round it, and AIC's answer to that route is unmeasured. A
 * {@code Context} (or a {@code ContextFactory}) in script hands over the
 * decompiler and fresh unguarded standard scopes, which render the emulator's
 * own installed functions as emulator source.
 *
 * <p>The seam is the wrap factory, not the class shutter. Every Java value
 * Rhino hands to a script — a method result, a field, a {@code new}, an
 * adapter argument — is wrapped here, so the check is by <em>type</em>
 * ({@code isInstance}: subclasses too) and covers routes no one has thought of.
 * The shutter only sees class names, and its engine-package exception cannot
 * tell an instance the engine needs from one a script was given.
 *
 * <p>Only capability classes are guarded. {@code ConsString}, {@code
 * EcmaError}, {@code NativeObject} and the rest are never refused: they are
 * what the engine itself hands over. Compiled {@code Script} objects are left
 * alone because {@code Script.exec} needs a {@code Context}, which this
 * refuses; {@code Function} is the language's ordinary function type.
 *
 * <p>A refusal is a tallied harness fault and a catchable error, so a script
 * that catches it still fails the run.
 */
final class EngineGuard extends WrapFactory {
  private final HarnessFaults faults;
  private final AmHostBridge bridge;

  EngineGuard(HarnessFaults faults, AmHostBridge bridge) {
    this.faults = faults;
    this.bridge = bridge;
  }

  @Override
  public Scriptable wrapAsJavaObject(Context cx, Scriptable scope, Object value, Class<?> staticType) {
    String kind = null;
    if (value instanceof Context) kind = "Context";
    else if (value instanceof ContextFactory) kind = "ContextFactory";
    if (kind != null) {
      String id = "engine-api-unmeasured/" + kind;
      String message =
          "an instance of " + value.getClass().getName() + " (a Rhino " + kind
              + ") reached the script. AIC hides the engine package by name; this route to it"
              + " is not measured, and the emulator's own functions cannot answer for it";
      if (bridge != null) throw bridge.missing(message, id);
      faults.record(id, "rhino-local: " + message);
      throw new EvaluatorException("rhino-local: " + id + ": " + message);
    }
    return super.wrapAsJavaObject(cx, scope, value, staticType);
  }
}
