import org.mozilla.javascript.Context;
import org.mozilla.javascript.ContextFactory;

/**
 * The Rhino context factory the runner uses.
 *
 * <ul>
 *   <li>{@code setOptimizationLevel(-1)} and {@code
 *       setInstructionObserverThreshold(1000)}. Language version is a per-job
 *       choice (AIC's measured JS is {@code VERSION_DEFAULT}).
 *   <li>{@code observeInstructionCount}: if the configured timeout has elapsed
 *       since {@code startTime}, throw {@code new Error("Interrupt.")}
 *   <li>{@code hasFeature(21)} is true ({@code FEATURE_ENABLE_JAVA_MAP_ACCESS})
 *   <li>{@code setMaximumInterpreterStackDepth(10000)} on every context.
 * </ul>
 *
 * <p>Timeout is per-job milliseconds, so one runaway script cannot take the
 * runner down. {@code 0} means no timeout.
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
    return new TimedContext(this);
  }

  @Override
  protected void observeInstructionCount(Context cx, int instructionCount) {
    TimedContext observed = (TimedContext) cx;
    long timeoutMs = observed.timeoutMs;
    if (timeoutMs <= 0) {
      return;
    }
    if (System.currentTimeMillis() - observed.startTime > timeoutMs) {
      if (observed.hostBridge != null) throw observed.hostBridge.state.timeout();
      throw new Error("Interrupt.");
    }
  }

  static final class TimedContext extends Context {
    final long startTime;
    long timeoutMs;
    AmHostBridge hostBridge;

    TimedContext(ContextFactory factory) {
      super(factory);
      this.startTime = System.currentTimeMillis();
      setOptimizationLevel(-1);
      setInstructionObserverThreshold(1000);
      setMaximumInterpreterStackDepth(10000);
    }
  }
}
