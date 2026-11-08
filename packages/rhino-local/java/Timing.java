import java.util.LinkedHashMap;
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
 */
final class Timing {
  private static final ThreadLocal<Timing> CURRENT = new ThreadLocal<Timing>();

  private final Map<String, Long> nanos = new LinkedHashMap<String, Long>();
  private final Map<String, Long> counts = new LinkedHashMap<String, Long>();
  private final long started = System.nanoTime();

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
    Map<String, Object> out = new LinkedHashMap<String, Object>();
    out.put("totalMs", ms(System.nanoTime() - started));
    for (Map.Entry<String, Long> e : nanos.entrySet()) out.put(e.getKey() + "Ms", ms(e.getValue()));
    for (Map.Entry<String, Long> e : counts.entrySet()) out.put(e.getKey(), e.getValue());
    return out;
  }
}
