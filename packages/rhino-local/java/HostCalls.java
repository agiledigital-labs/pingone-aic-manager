import java.util.LinkedHashMap;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.mozilla.javascript.WrappedException;

/** Reader-to-worker reply routing. A blocked call never owns the stdin reader. */
public final class HostCalls {
  private static final Map<String, HostCalls> JOBS = new ConcurrentHashMap<String, HostCalls>();
  private final Map<Integer, CompletableFuture<Map<String, Object>>> waiting =
      new ConcurrentHashMap<Integer, CompletableFuture<Map<String, Object>>>();
  private static volatile boolean inputClosed;
  private final String id;
  private final IdmContextFactory factory;
  IdmValues values;
  private int sequence;
  private final java.util.SortedMap<String, Map<String, Object>> evidence = new java.util.TreeMap<String, Map<String, Object>>();
  private synchronized void collectEvidence(Object raw, boolean failOnMissing) {
    if (raw == null) return;
    try {
      if (!(raw instanceof java.util.List)) throw new IllegalArgumentException("expected array");
      java.util.List<String> absent = new java.util.ArrayList<String>();
      for (Object item : (java.util.List<?>) raw) {
        Map<String, Object> entry = Json.asObject(item, "host evidence");
        String kind = Json.optionalString(entry, "kind"), behavior = Json.optionalString(entry, "behavior");
        String source = Json.optionalString(entry, "source");
        if (!("measured".equals(kind) || "assumed".equals(kind) || "missing".equals(kind)) || behavior == null) throw new IllegalArgumentException("expected evidence kind/behavior");
        String key = Json.stringify(java.util.Arrays.asList(kind, behavior, source));
        evidence.put(key, new LinkedHashMap<String, Object>(entry));
        if ("missing".equals(kind) && !absent.contains(behavior)) absent.add(behavior);
      }
      // Even an adapter that returns a failed REST body's JSON cannot demote a
      // missing record to tenant data merely by ignoring the result status.
      if (failOnMissing && !absent.isEmpty()) {
        EmulationFailure error = new EmulationFailure("missing", "Missing IDM behavior: " + String.join(", ", absent));
        error.info.put("missing", absent);
        throw fail(error);
      }
    } catch (RuntimeException e) {
      throw fail(new EmulationFailure("protocol", "rhino-local: invalid host evidence: " + e.getMessage()));
    }
  }
  /** Evidence the AM bridge records itself, e.g. use of a binding measured in docs/api. */
  synchronized void note(String kind, String behavior, String source) {
    Map<String, Object> entry = new LinkedHashMap<String, Object>();
    entry.put("kind", kind);
    entry.put("behavior", behavior);
    if (source != null) entry.put("source", source);
    evidence.put(Json.stringify(java.util.Arrays.asList(kind, behavior, source)), entry);
  }
  synchronized java.util.List<Object> evidenceSnapshot() { return new java.util.ArrayList<Object>(evidence.values()); }
  final java.util.Set<Integer> unanswered = new java.util.concurrent.ConcurrentSkipListSet<Integer>();
  EmulationFailure fail(EmulationFailure error) {
    return factory.fail(error);
  }
  final java.util.Set<String> missing = new java.util.concurrent.ConcurrentSkipListSet<String>();

  HostCalls(String id, IdmContextFactory factory) {
    this.id = id;
    this.factory = factory;
    if (JOBS.putIfAbsent(id, this) != null) {
      throw new IllegalArgumentException("duplicate active job id: " + id);
    }
  }

  Object call(String method, Object args) {
    if (inputClosed) throw fail(new EmulationFailure("protocol", "rhino-local: host input closed"));
    final int seq;
    CompletableFuture<Map<String, Object>> future = new CompletableFuture<Map<String, Object>>();
    // Allocation, registration and emission share one lock; never hold it while waiting.
    synchronized (this) {
      seq = ++sequence;
      waiting.put(seq, future);
      unanswered.add(seq);
      if (inputClosed) future.completeExceptionally(new IllegalStateException("rhino-local: host input closed"));
      Map<String, Object> line = new LinkedHashMap<String, Object>();
      line.put("type", "call");
      line.put("id", id);
      line.put("seq", seq);
      line.put("method", method);
      line.put("args", args);
      Runner.write(line);
    }
    Timing.count("hostCalls");
    long waitStarted = Timing.now();
    try {
      long remaining = factory.remainingMillis();
      Map<String, Object> reply;
      try { reply = remaining == 0 ? future.get() : future.get(remaining, TimeUnit.MILLISECONDS); }
      finally { Timing.add("hostWait", waitStarted); }
      unanswered.remove(seq); // Authoritative: this worker consumed the reply, not merely received stdin.
      // A failed reply's own emulation map wins over one rebuilt from its missing evidence:
      // it carries the harness-fault ids (`faults`), which the behaviour strings do not.
      Object replyError = reply.get("error");
      boolean emulated = replyError instanceof Map && ((Map<?, ?>) replyError).get("emulation") instanceof Map;
      collectEvidence(reply.get("provenance"), !emulated);
      if (reply.get("error") != null) {
        Map<String, Object> error = Json.asObject(reply.get("error"), "host error");
        java.util.List<String> absent = Json.optionalStringList(error, "missing");
        if (absent != null) missing.addAll(absent);
        if (error.get("emulation") instanceof Map) throw fail(new EmulationFailure(Json.asObject(error.get("emulation"), "emulation failure")));
        if (absent != null && !absent.isEmpty()) {
          EmulationFailure fatal = new EmulationFailure("missing", "Missing IDM behavior: " + String.join(", ", absent));
          fatal.info.put("missing", absent);
          throw fail(fatal);
        }
        throw new WrappedException(new HostException(error, values, this));
      }
      return reply.get("result");
    } catch (TimeoutException e) {
      throw factory.timeout();
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
      throw factory.timeout();
    } catch (ExecutionException e) {
      throw fail(new EmulationFailure("protocol", "rhino-local: host input closed"));
    } finally {
      waiting.remove(seq);
    }
  }

  static void reply(Map<String, Object> line) {
    Object id = line.get("id");
    Object seq = line.get("seq");
    if (!(id instanceof String) || !(seq instanceof Number)) return;
    HostCalls job = JOBS.get((String) id);
    if (job == null) return;
    CompletableFuture<Map<String, Object>> future = job.waiting.get(((Number) seq).intValue());
    if (future != null) future.complete(line);
  }

  void close() {
    JOBS.remove(id, this);
  }

  static void endInput() {
    inputClosed = true;
    for (HostCalls job : JOBS.values()) {
      for (CompletableFuture<Map<String, Object>> future : job.waiting.values()) {
        future.completeExceptionally(new IllegalStateException("rhino-local: host input closed"));
      }
    }
  }

  /**
   * The tenant exception a script catches as {@code e.javaException}. Rhino exposes it through
   * {@link IdmExceptionView} (IdmContextFactory's wrap factory), which admits only the members
   * measured on AIC and faults the rest: the real object is a CREST ResourceException subclass,
   * and this class is not one, so its other members would answer for the wrong class.
   */
  public static final class HostException extends Exception {
    /** The harness-fault id an unmeasured member or rendering of a caught exception faults under. */
    static final String FAULT = "idm-exception/unmeasured";
    private final IdmValues values;
    private final HostCalls host;
    private final String exceptionClass;
    private final String reason;
    private final Object code;
    private final Object detail;

    HostException(Map<String, Object> error, IdmValues values, HostCalls host) {
      super(Json.optionalString(error, "message"));
      this.values = values;
      this.host = host;
      exceptionClass = Json.optionalString(error, "class");
      reason = Json.optionalString(error, "reason");
      code = error.get("code");
      detail = error.get("detail");
    }

    public int getCode() { return code instanceof Number ? ((Number) code).intValue() : 500; }
    /** A Java null when the error carries none (measured for a 404: {@code String(getDetail())} is "null"). */
    public Object getDetail() {
      return detail == null ? null : values.wrapDetail(detail, this);
    }
    /** CREST's reason phrase (measured "Not Found" for a 404); an error that names none faults. */
    public String getReason() {
      if (reason == null) throw unmeasured("getReason() of an exception that carries no reason phrase");
      return reason;
    }
    String exceptionClass() { return exceptionClass; }
    Object detail() { return detail; }
    @Override public String toString() { return exceptionClass + ": " + getMessage(); }

    EmulationFailure unmeasured(String behavior) {
      String cause = FAULT + ": " + behavior;
      EmulationFailure error = new EmulationFailure("missing", "Missing IDM behavior: " + cause);
      error.info.put("missing", java.util.Collections.singletonList(cause));
      Map<String, Object> fault = new LinkedHashMap<String, Object>();
      fault.put("id", FAULT);
      fault.put("message", behavior);
      error.info.put("faults", java.util.Collections.singletonList(fault));
      return host.fail(error);
    }
  }
}
