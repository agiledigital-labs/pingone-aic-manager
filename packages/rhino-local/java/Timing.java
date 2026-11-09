import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Opt-in per-job phase timing (AIC_SCRIPT_TESTER_TIMING on the client side).
 *
 * <p>A job that carries {@code "timing": true} gets one of these on its worker
 * thread for the life of the job; every phase site adds to it through the
 * static helpers, which do nothing when no timer is installed, so a job that
 * did not ask pays one ThreadLocal read per site. The client strips the
 * {@code timing} member before anything compares or returns the response, so
 * timing never reaches a test's assertions or the lane comparison.
 *
 * <p>Phases accumulate: a job that loads three libraries reports their summed
 * compile time. {@code subjectExec} and {@code preambleExec} include any
 * library module bodies they run and any host-call wait ({@code hostWait}
 * says how much of that was waiting on the client).
 *
 * <p>Phases the runtime itself brackets ({@link PhaseTimer}): {@code moduleBody}
 * (a library's module body, run when a script first requires it) and
 * {@code harvest} (collecting a case's result). They nest — a module body that
 * requires another contains it — so {@code moduleBodyMs} counts only the
 * outermost body of each nest, and {@code moduleBodies} gives each library its
 * <em>self</em> time (less the bodies it required), which sum to
 * {@code moduleBodyMs}. {@code subjectOwn} is subject execution less the
 * module bodies and harvest that ran inside it.
 */
final class Timing {
  private static final ThreadLocal<Timing> CURRENT = new ThreadLocal<Timing>();

  private final Map<String, Long> nanos = new LinkedHashMap<String, Long>();
  private final Map<String, Long> counts = new LinkedHashMap<String, Long>();
  private final long started = System.nanoTime();
  /** Self nanoseconds per library id, in first-run order. */
  private final Map<String, Long> modules = new LinkedHashMap<String, Long>();
  /** Bracketed phases still open, innermost last. */
  private final List<Frame> frames = new ArrayList<Frame>();

  private static final class Frame {
    final String phase;
    final String label;
    final long start = System.nanoTime();
    long children;

    Frame(String phase, String label) {
      this.phase = phase;
      this.label = label;
    }
  }

  /** Installs a timer on this thread when the job asked for one; returns it, or null. */
  static Timing begin(Map<String, Object> job) {
    if (!Boolean.TRUE.equals(job.remove("timing"))) return null;
    Timing timing = new Timing();
    Object read = job.remove("timingReadNanos");
    if (read instanceof Number) timing.nanos.put("readerDecode", ((Number) read).longValue());
    Object bytes = job.remove("timingSourceBytes");
    if (bytes instanceof Number) timing.counts.put("sourceCharsRegistered", ((Number) bytes).longValue());
    CURRENT.set(timing);
    return timing;
  }

  static void end() {
    CURRENT.remove();
  }

  static long now() {
    return CURRENT.get() == null ? 0L : System.nanoTime();
  }

  /** Adds the time since {@code since} (a {@link #now()} value) to {@code phase}. */
  static void add(String phase, long since) {
    Timing timing = CURRENT.get();
    if (timing == null || since == 0L) return;
    timing.nanos.merge(phase, System.nanoTime() - since, Long::sum);
  }

  /** {@link #add}, less {@code nested} nanoseconds that another phase already accounts for. */
  static void addLess(String phase, long since, long nested) {
    Timing timing = CURRENT.get();
    if (timing == null || since == 0L) return;
    timing.nanos.merge(phase, System.nanoTime() - since - nested, Long::sum);
  }

  /** The nanoseconds {@code phase} holds so far (0 with no timer installed). */
  static long total(String phase) {
    Timing timing = CURRENT.get();
    if (timing == null) return 0L;
    Long n = timing.nanos.get(phase);
    return n == null ? 0L : n.longValue();
  }

  /** True when this thread's job asked for timing. */
  static boolean active() {
    return CURRENT.get() != null;
  }

  /**
   * Opens a bracketed phase; returns the depth to hand to {@link #exit}. A
   * frame left open (its script threw past the close, or timed out) is closed
   * by the next exit below it, or when the job's row is written.
   */
  static int enter(String phase, String label) {
    Timing timing = CURRENT.get();
    if (timing == null) return -1;
    timing.frames.add(new Frame(phase, label));
    return timing.frames.size() - 1;
  }

  static void exit(int depth) {
    Timing timing = CURRENT.get();
    if (timing == null || depth < 0) return;
    timing.closeTo(depth, System.nanoTime());
  }

  private void closeTo(int depth, long now) {
    while (frames.size() > depth) {
      Frame frame = frames.remove(frames.size() - 1);
      long elapsed = now - frame.start;
      boolean nested = false;
      for (Frame outer : frames) if (outer.phase.equals(frame.phase)) nested = true;
      if (!frames.isEmpty()) frames.get(frames.size() - 1).children += elapsed;
      if (!nested) nanos.merge(frame.phase, elapsed, Long::sum);
      counts.merge(frame.phase + "Runs", 1L, Long::sum);
      if (frame.label != null) modules.merge(frame.label, elapsed - frame.children, Long::sum);
    }
  }

  static void count(String name) {
    Timing timing = CURRENT.get();
    if (timing == null) return;
    timing.counts.merge(name, 1L, Long::sum);
  }

  /** Milliseconds to three places, so a sub-millisecond phase is not reported as zero. */
  private static Double ms(long n) {
    return Double.valueOf(Math.round(n / 1000.0) / 1000.0);
  }

  Map<String, Object> toJson() {
    long now = System.nanoTime();
    closeTo(0, now);
    Map<String, Object> out = new LinkedHashMap<String, Object>();
    out.put("totalMs", ms(now - started));
    for (Map.Entry<String, Long> e : nanos.entrySet()) out.put(e.getKey() + "Ms", ms(e.getValue()));
    for (Map.Entry<String, Long> e : counts.entrySet()) out.put(e.getKey(), e.getValue());
    if (!modules.isEmpty()) {
      Map<String, Object> bodies = new LinkedHashMap<String, Object>();
      for (Map.Entry<String, Long> e : modules.entrySet()) bodies.put(e.getKey(), ms(e.getValue()));
      out.put("moduleBodies", bodies);
    }
    return out;
  }
}
