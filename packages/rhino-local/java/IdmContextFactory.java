import java.util.Locale;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.ContextFactory;

/** IDM's plain feature set, with an optional harness deadline in compiled code. */
final class IdmContextFactory extends ContextFactory {
  private final long timeoutMs;
  private final long started = System.nanoTime();

  IdmContextFactory(long timeoutMs) {
    this.timeoutMs = timeoutMs;
  }

  @Override
  protected Context makeContext() {
    Context cx = super.makeContext();
    configure(cx, timeoutMs);
    return cx;
  }

  Context enterIdmContext() {
    if (timeoutMs > 0) return enterContext();
    Context cx = new ContextFactory().enterContext();
    configure(cx, 0);
    return cx;
  }

  private static void configure(Context cx, long timeoutMs) {
    cx.setLanguageVersion(200);
    cx.setOptimizationLevel(9);
    cx.setLocale(Locale.ENGLISH);
    // Rhino emits observer checks into JVM bytecode when this is nonzero.
    cx.setInstructionObserverThreshold(timeoutMs > 0 ? 1000 : 0);
  }

  long remainingMillis() {
    return timeoutMs <= 0 ? 0 : Math.max(1, timeoutMs - (System.nanoTime() - started) / 1_000_000);
  }

  @Override
  protected void observeInstructionCount(Context cx, int count) {
    if (timeoutMs > 0 && (System.nanoTime() - started) / 1_000_000 > timeoutMs) {
      throw new Error("Interrupt.");
    }
  }
}
