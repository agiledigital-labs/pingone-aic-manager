import java.util.Locale;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.ContextFactory;
import org.mozilla.javascript.Function;
import org.mozilla.javascript.Script;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptRuntime;

/** IDM's plain feature set, with an optional harness deadline in compiled code. */
final class IdmContextFactory extends ContextFactory {
  private final long timeoutMs;
  private final long started = System.nanoTime();
  // One state for every context/thread executing this job, including shell spawn.
  volatile EmulationFailure failure;
  private volatile boolean finished;
  private final java.util.List<Thread> spawned = new java.util.ArrayList<Thread>();

  /** Shell spawn calling convention, with admission recorded before thread start. */
  Object spawn(Context parent, Scriptable scope, Object[] args, Function shellSpawn) {
    if (args.length == 0 || !(args[0] instanceof Function || args[0] instanceof Script)) {
      return shellSpawn.call(parent, scope, scope, args); // Shell's own argument error.
    }
    final Object target = args[0];
    final Object[] argv = args.length > 1 && args[1] instanceof Scriptable
        ? parent.getElements((Scriptable) args[1]) : ScriptRuntime.emptyArgs;
    Thread thread = new Thread(() -> {
      try {
        call(cx -> target instanceof Function ? ((Function) target).call(cx, scope, scope, argv)
            : ((Script) target).exec(cx, scope));
      } catch (EmulationFailure e) {
        fail(e); // Child failure remains sticky even when nobody joins it.
      }
    });
    thread.setDaemon(true); // A refused detached child must not keep shutdown alive.
    synchronized (this) {
      if (finished) throw detached();
      spawned.add(thread);
      thread.start();
    }
    return Context.javaToJS(thread, scope);
  }

  private EmulationFailure detached() {
    EmulationFailure error = new EmulationFailure("missing", "Missing IDM behavior: spawned thread outlived its job");
    error.info.put("missing", java.util.Collections.singletonList("spawned thread outlived its job"));
    return fail(error);
  }

  void finish() {
    java.util.List<Thread> live = new java.util.ArrayList<Thread>();
    synchronized (this) {
      finished = true;
      for (Thread thread : spawned) if (thread.isAlive()) live.add(thread);
      if (!live.isEmpty()) detached();
    }
    // Stop script observers and Java waits without holding the failure-state lock.
    for (Thread thread : live) thread.interrupt();
    long until = System.nanoTime() + 100_000_000L;
    for (Thread thread : live) {
      long remaining = (until - System.nanoTime()) / 1_000_000;
      if (remaining <= 0) break;
      try { thread.join(remaining); }
      catch (InterruptedException e) { Thread.currentThread().interrupt(); break; }
    }
  }

  synchronized EmulationFailure fail(EmulationFailure error) {
    if (failure == null) failure = error;
    return failure;
  }

  EmulationFailure timeout() {
    return fail(new EmulationFailure("timeout", "IDM harness timeout"));
  }

  IdmContextFactory(long timeoutMs) {
    this.timeoutMs = timeoutMs;
  }

  @Override
  protected Context makeContext() {
    Context cx = super.makeContext();
    configure(cx, timeoutMs);
    // Children need lifetime checks even when the parent disabled its deadline.
    cx.setInstructionObserverThreshold(1000);
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
    if (finished) throw detached();
    if (timeoutMs > 0 && (System.nanoTime() - started) / 1_000_000 > timeoutMs) {
      throw timeout();
    }
  }
}
