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
  private int sequence;

  HostCalls(String id, IdmContextFactory factory) {
    this.id = id;
    this.factory = factory;
    if (JOBS.putIfAbsent(id, this) != null) {
      throw new IllegalArgumentException("duplicate active job id: " + id);
    }
  }

  Object call(String method, Object args) {
    if (inputClosed) throw new IllegalStateException("rhino-local: host input closed");
    int seq = ++sequence;
    CompletableFuture<Map<String, Object>> future = new CompletableFuture<Map<String, Object>>();
    waiting.put(seq, future);
    if (inputClosed) future.completeExceptionally(new IllegalStateException("rhino-local: host input closed"));
    Map<String, Object> line = new LinkedHashMap<String, Object>();
    line.put("type", "call");
    line.put("id", id);
    line.put("seq", seq);
    line.put("method", method);
    line.put("args", args);
    Runner.write(line);
    try {
      long remaining = factory.remainingMillis();
      Map<String, Object> reply = remaining == 0 ? future.get() : future.get(remaining, TimeUnit.MILLISECONDS);
      if (reply.get("error") != null) {
        Map<String, Object> error = Json.asObject(reply.get("error"), "host error");
        throw new WrappedException(new HostException(error));
      }
      return reply.get("result");
    } catch (TimeoutException e) {
      throw new Error("Interrupt.");
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
      throw new Error("Interrupt.");
    } catch (ExecutionException e) {
      throw new WrappedException(e.getCause());
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
    private final String exceptionClass;
    private final Object code;
    private final Object detail;

    HostException(Map<String, Object> error) {
      super(Json.optionalString(error, "message"));
      exceptionClass = Json.optionalString(error, "class");
      code = error.get("code");
      detail = error.get("detail");
    }

    public int getCode() { return code instanceof Number ? ((Number) code).intValue() : 500; }
    public Object getDetail() {
      return IdmValues.wrapCurrent(detail);
    }
    String exceptionClass() { return exceptionClass; }
    Object detail() { return detail; }
    @Override public String toString() { return exceptionClass + ": " + getMessage(); }
  }
}
