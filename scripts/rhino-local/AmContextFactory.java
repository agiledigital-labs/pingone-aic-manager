import org.mozilla.javascript.Context;
import org.mozilla.javascript.ContextFactory;

/**
 * Replica of AM 8.1.1 {@code ObservedContextFactory} + its inner {@code
 * ObservedJavaScriptContext}, plus the {@code
 * RhinoScriptEngineFactory.getContext} knobs that constructor does not set.
 *
 * <p>Measured:
 *
 * <ul>
 *   <li>{@code makeContext} constructs {@code ObservedJavaScriptContext}
 *   <li>that constructor: {@code setOptimizationLevel(-1)}, {@code
 *       setInstructionObserverThreshold(1000)}, and {@code
 *       setLanguageVersion(200)} only when {@code
 *       org.forgerock.am.scripting.disableES6} is false. Language version is
 *       a per-job choice here (AIC's measured JS is {@code VERSION_DEFAULT}).
 *   <li>{@code observeInstructionCount}: if configured timeout (AM stores it
 *       in <em>seconds</em>) has elapsed since {@code startTime}, throw {@code
 *       new Error("Interrupt.")}
 *   <li>{@code hasFeature(21)} is true ({@code FEATURE_ENABLE_JAVA_MAP_ACCESS})
 *   <li>{@code getContext} also {@code setMaximumInterpreterStackDepth} (default
 *       10000). We set that on every context.
 * </ul>
 *
 * <p>Timeout is per-job milliseconds rather than AM's process-wide seconds, so
 * one runaway script cannot take the runner down. {@code 0} means no timeout,
 * matching AM's {@code ScriptEngineConfiguration.NO_TIMEOUT}.
 */
final class AmContextFactory extends ContextFactory {

  @Override
  protected boolean hasFeature(Context cx, int featureIndex) {
    if (featureIndex == Context.FEATURE_ENABLE_JAVA_MAP_ACCESS) {
      return true;
    }
    return super.hasFeature(cx, featureIndex);
  }

  @Override
  protected Context makeContext() {
    return new ObservedContext(this);
  }

  @Override
  protected void observeInstructionCount(Context cx, int instructionCount) {
    ObservedContext observed = (ObservedContext) cx;
    long timeoutMs = observed.timeoutMs;
    if (timeoutMs <= 0) {
      return;
    }
    if (System.currentTimeMillis() - observed.startTime > timeoutMs) {
      throw new Error("Interrupt.");
    }
  }

  static final class ObservedContext extends Context {
    final long startTime;
    long timeoutMs;

    ObservedContext(ContextFactory factory) {
      super(factory);
      this.startTime = System.currentTimeMillis();
      setOptimizationLevel(-1);
      setInstructionObserverThreshold(1000);
      setMaximumInterpreterStackDepth(10000);
    }
  }
}
