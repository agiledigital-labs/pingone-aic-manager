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
  final java.util.Set<Integer> unanswered = new java.util.concurrent.ConcurrentSkipListSet<Integer>();
  volatile EmulationFailure failure;
  synchronized EmulationFailure fail(EmulationFailure error) {
    if (failure == null) failure = error;
    return failure;
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
    try {
      long remaining = factory.remainingMillis();
      Map<String, Object> reply = remaining == 0 ? future.get() : future.get(remaining, TimeUnit.MILLISECONDS);
      unanswered.remove(seq); // Authoritative: this worker consumed the reply, not merely received stdin.
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
        throw new WrappedException(new HostException(error, values));
      }
      return reply.get("result");
    } catch (TimeoutException e) {
      throw new Error("Interrupt.");
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
      throw new Error("Interrupt.");
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

  /** Public methods are intentionally visible through Rhino's JavaException wrapper. */
  public static final class HostException extends Exception {
    private final IdmValues values;
    private final String exceptionClass;
    private final Object code;
    private final Object detail;

    HostException(Map<String, Object> error, IdmValues values) {
      super(Json.optionalString(error, "message"));
      this.values = values;
      exceptionClass = Json.optionalString(error, "class");
      code = error.get("code");
      detail = error.get("detail");
    }

    public int getCode() { return code instanceof Number ? ((Number) code).intValue() : 500; }
    public Object getDetail() {
      return values.wrap(detail);
    }
    String exceptionClass() { return exceptionClass; }
    Object detail() { return detail; }
    @Override public String toString() { return exceptionClass + ": " + getMessage(); }
  }
}
