import java.io.BufferedReader;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TimeZone;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.SynchronousQueue;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import javax.script.ScriptContext;
import javax.script.SimpleScriptContext;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.EvaluatorException;
import org.mozilla.javascript.RhinoException;
import org.mozilla.javascript.Script;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;

/**
 * Long-lived JVM speaking line-delimited JSON on stdin/stdout. One process,
 * many jobs, a fresh {@link ScriptContextScope} per job. Anything the runner
 * itself needs to say goes to stderr.
 *
 * <pre>
 * java -cp rhino.jar:classes Runner
 * </pre>
 */
public final class Runner {

  /** Harness default so a missing timeout cannot hang the process. AM's default is 0 (no timeout). */
  static final long DEFAULT_TIMEOUT_MS = 10_000L;

  static final PrintStream OUT =
      new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8);
  static final PrintStream ERR =
      new PrintStream(new FileOutputStream(FileDescriptor.err), true, StandardCharsets.UTF_8);

  private static volatile boolean retired;
  /** Marker line: a pool that reuses JVMs across clients reads it to stop handing this one out. */
  static final String RETIRED_MARKER = "rhino-local-runner retired";
  static synchronized void retire() {
    if (retired) return;
    retired = true;
    ERR.println(RETIRED_MARKER);
  }

  private static final Map<String, Boolean> ACTIVE = new ConcurrentHashMap<String, Boolean>();

  /**
   * Sticky: this JVM has run a job with no class shutter (no
   * {@code classAllowList}, or the IDM engine, which has none). Such a job can
   * reach any Java class, so it can leave JVM-global state behind when it
   * finishes normally: a default timezone, a system property, a thread it
   * started. Nothing here can undo that, so the pool never lends this JVM to
   * another client once the flag is set.
   */
  private static volatile boolean unshuttered;

  static boolean shuttered(Map<String, Object> job) {
    Object engine = job.get("engine");
    boolean am = engine == null || "am".equals(engine);
    return am && job.get("classAllowList") instanceof List;
  }

  /**
   * The JVM pool asks this before lending a JVM to another client: a JVM with
   * a job still running, one that retired itself, or one that has run a job
   * with no class shutter must never be reused.
   * Read from the same flags that refuse jobs, so it cannot disagree with
   * them the way a scan of stderr for {@link #RETIRED_MARKER} can.
   */
  static final String POOL_STATUS_OP = "pool-status";

  private static Map<String, Object> poolStatus(String id) {
    Map<String, Object> status = new LinkedHashMap<String, Object>();
    status.put("type", POOL_STATUS_OP);
    status.put("id", id);
    status.put("active", Integer.valueOf(ACTIVE.size()));
    status.put("retired", Boolean.valueOf(retired));
    status.put("unshuttered", Boolean.valueOf(unshuttered));
    return status;
  }

  public static void main(String[] args) throws Exception {
    int maxJobs = 128;
    long shutdownWaitMs = 5000;
    for (String arg : args) {
      if (arg.startsWith("--max-concurrent-jobs=")) maxJobs = Integer.parseInt(arg.substring(arg.indexOf('=') + 1));
      else if (arg.startsWith("--shutdown-wait-ms=")) shutdownWaitMs = Long.parseLong(arg.substring(arg.indexOf('=') + 1));
      else throw new IllegalArgumentException("unknown runner option: " + arg);
    }
    if (maxJobs <= 0 || shutdownWaitMs <= 0) throw new IllegalArgumentException("runner limits must be positive");
    // Undefined caches an identity hash at class initialization. Initialize on
    // the reader, before workers race: its diagnostic text must agree across JVMs.
    Class.forName("org.mozilla.javascript.Undefined");
    // No job Context comes from the global factory, but the engine package is
    // visible to the engine and, with no shutter (raw runner, IDM), to scripts
    // by name, so a script could reach it and add a listener that outlives its
    // file. Install and seal one here: a sealed factory refuses listeners and class-loader changes, and
    // installing it makes a later initGlobal throw.
    org.mozilla.javascript.ContextFactory global = new org.mozilla.javascript.ContextFactory();
    org.mozilla.javascript.ContextFactory.initGlobal(global);
    global.seal();
    final AmContextFactory factory = new AmContextFactory();
    ERR.println("rhino-local-runner ready " + Json.stringify(environment(factory)));
    final BufferedReader in =
        new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
    // No queue: nested callers get a response instead of waiting behind blocked parents.
    final ThreadPoolExecutor workers = new ThreadPoolExecutor(0, maxJobs, 30, TimeUnit.SECONDS,
        new SynchronousQueue<Runnable>());
    boolean shuttingDown = false;
    String line;
    while ((line = in.readLine()) != null) {
      if (line.isEmpty()) {
        continue;
      }
      final Object parsed;
      final long decodeStarted = System.nanoTime();
      try {
        parsed = ProtocolValues.decode(Json.parse(line));
      } catch (RuntimeException e) {
        ERR.println("rhino-local-runner: skipping unparseable line: " + e.getMessage());
        continue;
      }
      if (parsed instanceof Map && "reply".equals(((Map<?, ?>) parsed).get("type"))) {
        HostCalls.reply(Json.asObject(parsed, "reply"));
      } else if (parsed instanceof Map) {
        final Map<String, Object> job = Json.asObject(parsed, "job");
        Object rawId = job.get("id");
        if (!(rawId instanceof String) || ((String) rawId).isEmpty()) {
          ERR.println("rhino-local-runner: skipping job with no string id");
          continue;
        }
        String id = (String) rawId;
        if (Boolean.TRUE.equals(job.get("timing"))) {
          job.put("timingReadNanos", Long.valueOf(System.nanoTime() - decodeStarted));
          long chars = 0;
          if (job.get("sources") instanceof Map)
            for (Object text : ((Map<?, ?>) job.get("sources")).values()) if (text instanceof String) chars += ((String) text).length();
          job.put("timingSourceBytes", Long.valueOf(chars));
        }
        // Before any refusal: the client counts a source as shipped once its
        // line is written, whatever becomes of the job that carried it.
        String sourceError = registerSources(job.remove("sources"));
        if (sourceError != null) {
          write(refusal(id, "rhino-local: " + sourceError));
        } else if (shuttingDown) {
          write(refusal(id, "rhino-local: runner is shutting down"));
        } else if (POOL_STATUS_OP.equals(job.get("op"))) {
          // Answered on the reader, so it cannot queue behind (or be refused like) a job.
          write(poolStatus(id));
        } else if (Boolean.TRUE.equals(job.get("shutdown")) || "shutdown".equals(job.get("op"))) {
          shuttingDown = true;
          shutdown(workers, shutdownWaitMs, id);
        } else if (retired) {
          write(refusal(id, "rhino-local: JVM retired because a spawned thread survived cleanup"));
        } else if (ACTIVE.putIfAbsent(id, Boolean.TRUE) != null) {
          write(refusal(id, "rhino-local: duplicate active job id"));
        } else {
          // Set before the job can run, on the reader, so a status line sent
          // after this job always sees it.
          if (!shuttered(job)) unshuttered = true;
          try { workers.execute(() -> handleJob(factory, job)); }
          catch (RejectedExecutionException e) {
            respond(refusal(id, "rhino-local: concurrent job limit exceeded (" + maxJobs + ")"));
          }
        }
      } else {
        ERR.println("rhino-local-runner: skipping non-object job");
      }
    }
    HostCalls.endInput();
    if (!shuttingDown) shutdown(workers, shutdownWaitMs, null);
  }

  private static Map<String, Object> refusal(String id, String message) {
    return errorResponse(id, "protocol_error", new IllegalStateException(message), null, -1, -1, null);
  }

  /** Exactly one job response, including jobs that exceed the shutdown deadline. */
  private static synchronized void respond(Map<String, Object> response) {
    if (ACTIVE.remove(response.get("id")) != null) write(response);
  }

  private static void shutdown(ThreadPoolExecutor workers, long waitMs, String id) {
    workers.shutdown();
    Thread drain = new Thread(() -> {
      boolean finished = false;
      try { finished = workers.awaitTermination(waitMs, TimeUnit.MILLISECONDS); }
      catch (InterruptedException e) { Thread.currentThread().interrupt(); }
      synchronized (Runner.class) {
        if (!finished) {
          // Respond before interrupting workers, so the bound has a deterministic reason.
          for (String activeId : ACTIVE.keySet()) respond(refusal(activeId, "rhino-local: shutdown wait exceeded"));
          workers.shutdownNow();
        }
        if (id != null) write(response(id, "ok", JsValues.of(Scriptable.NOT_FOUND), null));
      }
      System.exit(0);
    }, "rhino-local-shutdown");
    drain.start();
  }

  /**
   * What this JVM would make of a script that looked at its surroundings. The
   * client refuses a JVM whose answers differ from the expected ones, because a
   * host that defaulted to its own timezone or locale would pass a date-shaped
   * test locally that fails on the tenant.
   */
  static Map<String, Object> environment(AmContextFactory factory) {
    final Map<String, Object> env = new LinkedHashMap<String, Object>();
    env.put("javaFeature", Integer.valueOf(Runtime.version().feature()));
    env.put("javaVersion", Runtime.version().toString());
    env.put("javaVendorVersion", System.getProperty("java.vendor.version"));
    env.put("timezone", TimeZone.getDefault().getID());
    env.put("locale", Locale.getDefault().toLanguageTag());
    env.put("charset", Charset.defaultCharset().name());
    final Context cx = factory.enterContext();
    try {
      env.put("rhino", cx.getImplementationVersion());
    } finally {
      Context.exit();
    }
    return env;
  }

  /**
   * Content-addressed sources a job line carries: {@code {"<sha256>": "<text>"}}.
   * The client sends each text once per JVM and names it by hash afterwards,
   * because a 470 KB job line costs ~8 ms to move and parse on its own.
   */
  private static String registerSources(Object raw) {
    if (raw == null) return null;
    try {
      for (Map.Entry<String, Object> e : Json.asObject(raw, "sources").entrySet()) {
        if (!(e.getValue() instanceof String)) return "source " + e.getKey() + " is not a string";
        ScriptCache.register(e.getKey(), (String) e.getValue());
      }
      return null;
    } catch (RuntimeException e) {
      return e.getMessage();
    }
  }

  /** The job map the reader already parsed; parsing it again here cost as much as the move. */
  static void handleJob(AmContextFactory factory, Map<String, Object> job) {
    final String id = (String) job.get("id");
    final Timing timing = Timing.begin(job);
    Map<String, Object> result;
    try {
      Object engine = job.get("engine");
      if (engine == null || "am".equals(engine)) {
        result = evalJob(factory, id, job);
      } else if ("idm".equals(engine)) {
        result = IdmRuntime.eval(id, job);
      } else {
        result = errorResponse(id, "protocol_error", new IllegalArgumentException("unknown engine: " + engine), null, -1, -1, null);
      }
    } catch (Throwable t) {
      result = errorResponse(id, "runtime_error", t, null, -1, -1, null);
      result.put("emulation", new EmulationFailure("internal", t.toString()).info);
    } finally {
      Timing.end();
    }
    if (timing != null) result.put("timing", timing.toJson());
    respond(result);
  }

  static Map<String, Object> evalJob(
      AmContextFactory factory, String id, Map<String, Object> job) {
    if (!Boolean.TRUE.equals(job.get("hostCalls"))) return evalAmJob(factory, id, job, null);
    AmHostBridge bridge = new AmHostBridge(id, Json.optionalLong(job, "timeoutMs", DEFAULT_TIMEOUT_MS));
    Map<String, Object> result;
    try { result = evalAmJob(factory, id, job, bridge); }
    catch (EmulationFailure e) { bridge.state.fail(e); result = errorResponse(id, "runtime_error", e, null, -1, -1, null); }
    catch (Throwable e) { bridge.calls.close(); throw e; }
    return bridge.finish(id, result);
  }

  private static Map<String, Object> evalAmJob(
      AmContextFactory factory, String id, Map<String, Object> job, AmHostBridge bridge) {
    // A journey has its own uncatchable channel (AmHostBridge's unsupported), so
    // the recorder is the script tester's alone.
    HarnessFaults faults = bridge == null ? new HarnessFaults() : null;
    Map<String, Object> result = evalAmJob(factory, id, job, bridge, faults);
    return faults == null ? result : faults.attach(result);
  }

  private static Map<String, Object> evalAmJob(
      AmContextFactory factory, String id, Map<String, Object> job, AmHostBridge bridge,
      HarnessFaults faults) {
    final String source = Json.optionalString(job, "source");
    if (source == null) {
      return errorResponse(
          id,
          "protocol_error",
          new IllegalArgumentException("job is missing source"),
          null,
          -1,
          -1,
          null);
    }
    final String sourceName;
    final String requestedName = Json.optionalString(job, "sourceName");
    if (requestedName != null && !requestedName.isEmpty()) {
      sourceName = requestedName;
    } else {
      sourceName = id;
    }
    final int languageVersion;
    try {
      languageVersion = LanguageVersions.parse(job.get("languageVersion"));
    } catch (RuntimeException e) {
      return errorResponse(id, "protocol_error", e, sourceName, -1, -1, null);
    }
    final long timeoutMs = Json.optionalLong(job, "timeoutMs", DEFAULT_TIMEOUT_MS);
    final String preamble = Json.optionalString(job, "preamble");
    final String preambleName;
    final String requestedPreamble = Json.optionalString(job, "preambleName");
    if (requestedPreamble != null && !requestedPreamble.isEmpty()) {
      preambleName = requestedPreamble;
    } else {
      preambleName = "<preamble>";
    }

    final List<String> classAllowList;
    try {
      classAllowList = Json.optionalStringList(job, "classAllowList");
    } catch (RuntimeException e) {
      return errorResponse(id, "protocol_error", e, sourceName, -1, -1, null);
    }

    final String preludeHash;
    final String preambleData;
    final Map<String, String> libraryRefs;
    try {
      preludeHash = Json.optionalString(job, "prelude");
      preambleData = Json.optionalString(job, "preambleData");
      libraryRefs = stringMap(job.get("libraryRefs"), "libraryRefs");
      if (preludeHash != null) ScriptCache.source(preludeHash);
      if (libraryRefs != null) for (String hash : libraryRefs.values()) ScriptCache.source(hash);
    } catch (RuntimeException e) {
      return errorResponse(id, "protocol_error", e, sourceName, -1, -1, null);
    }

    long phase = Timing.now();
    final Context cx = factory.enterContext();
    try {
      cx.setLanguageVersion(languageVersion);
      cx.setLocale(Locale.US);
      // No list means no shutter, which is the pre-shutter behaviour: every
      // Java name resolves. Callers that mean to reproduce a script context
      // pass that context's allowLists.
      if (classAllowList != null) {
        cx.setClassShutter(AmClassShutter.of(classAllowList));
      }
      if (cx instanceof AmContextFactory.TimedContext) {
        ((AmContextFactory.TimedContext) cx).timeoutMs = timeoutMs;
        ((AmContextFactory.TimedContext) cx).hostBridge = bridge;
      }
      final SimpleScriptContext scriptContext = AmScopes.newEngineContext(sourceName);
      injectGlobals(scriptContext, job.get("globals"));
      final Scriptable scope = AmScopes.makeScriptable(cx, scriptContext);
      Timing.add("setup", phase);

      final boolean hasPreamble = preamble != null && !preamble.isEmpty();
      if (hasPreamble || preludeHash != null) {
        try {
          final String hostOpName = "__rhinoLocalHostOp";
          HostOps hostOps = new HostOps();
          hostOps.setParentScope(scope);
          hostOps.setPrototype(ScriptableObject.getFunctionPrototype(scope));
          ScriptableObject.putProperty(scope, hostOpName, hostOps);
          if (faults != null) {
            faults.setParentScope(scope);
            faults.setPrototype(ScriptableObject.getFunctionPrototype(scope));
            ScriptableObject.putProperty(scope, HarnessFaults.NAME, faults);
          }
          if (bridge != null) {
            bridge.setParentScope(scope);
            bridge.setPrototype(ScriptableObject.getFunctionPrototype(scope));
            ScriptableObject.putProperty(scope, "__rhinoLocalJourneyHost", bridge);
          }
          if (libraryRefs != null) {
            LibraryHost libraries = new LibraryHost(libraryRefs);
            libraries.setParentScope(scope);
            libraries.setPrototype(ScriptableObject.getFunctionPrototype(scope));
            ScriptableObject.putProperty(scope, LIBRARIES_NAME, libraries);
          }
          if (preambleData != null) ScriptableObject.putProperty(scope, PREAMBLE_DATA_NAME, preambleData);
          NativeMethod.Factory nativeMethods = new NativeMethod.Factory();
          nativeMethods.setParentScope(scope);
          nativeMethods.setPrototype(ScriptableObject.getFunctionPrototype(scope));
          ScriptableObject.putProperty(scope, NativeMethod.FACTORY_NAME, nativeMethods);
          if (Timing.active()) {
            PhaseTimer timer = new PhaseTimer();
            timer.setParentScope(scope);
            timer.setPrototype(ScriptableObject.getFunctionPrototype(scope));
            ScriptableObject.putProperty(scope, PhaseTimer.NAME, timer);
          }
          try {
            // The prelude and the preamble run as one script would: the
            // preamble continues the prelude's line numbering under the same
            // name, so a position reported from either is where it was when
            // they were sent as one string.
            int line = 1;
            if (preludeHash != null) {
              final ScriptCache.Source prelude = ScriptCache.source(preludeHash);
              // Every started phase is recorded in a finally, so one that threw
              // or timed out still reports the time it took.
              Script compiled;
              phase = Timing.now();
              try { compiled = ScriptCache.compile(cx, "prelude:" + preludeHash, () -> prelude.text, preambleName, 1); }
              finally { Timing.add("preludeCompile", phase); }
              phase = Timing.now();
              try { compiled.exec(cx, scope); }
              finally { Timing.add("preludeExec", phase); }
              line = prelude.lines + 1;
            }
            if (hasPreamble) {
              Script preambleScript;
              phase = Timing.now();
              try { preambleScript = cx.compileString(preamble, preambleName, line, null); }
              finally { Timing.add("preambleCompile", phase); }
              phase = Timing.now();
              try { preambleScript.exec(cx, scope); }
              finally { Timing.add("preambleExec", phase); }
            }
          } finally {
            ScriptableObject.deleteProperty(scope, hostOpName);
            ScriptableObject.deleteProperty(scope, LIBRARIES_NAME);
            ScriptableObject.deleteProperty(scope, PREAMBLE_DATA_NAME);
            ScriptableObject.deleteProperty(scope, PhaseTimer.NAME);
            ScriptableObject.deleteProperty(scope, NativeMethod.FACTORY_NAME);
            ScriptableObject.deleteProperty(scope, HarnessFaults.NAME);
            if (bridge != null) ScriptableObject.deleteProperty(scope, "__rhinoLocalJourneyHost");
          }
          ((ScriptContextScope) scope).sealHarnessGlobals();
        } catch (EvaluatorException e) {
          return errorFromRhino(id, "compile_error", e);
        } catch (RhinoException e) {
          return errorFromRhino(id, "runtime_error", e);
        } catch (Error e) {
          if (isInterrupt(e)) {
            return timeoutResponse(id);
          }
          throw e;
        }
      }

      final Script script;
      phase = Timing.now();
      try {
        // Cached like the prelude and the libraries: the key carries the
        // source's hash, its name and every context setting the compiler
        // reads, and only the compiled form is shared, never a scope.
        script = ScriptCache.compile(cx, "subject:" + ScriptCache.sha256(source), () -> source, sourceName, 1, "subject");
      } catch (EvaluatorException e) {
        return errorFromRhino(id, "compile_error", e);
      } catch (RhinoException e) {
        return errorFromRhino(id, "compile_error", e);
      } finally {
        Timing.add("subjectCompile", phase);
      }

      final Object completion;
      phase = Timing.now();
      // The subject's own-time accounting runs only for a timed job; untimed,
      // the added work is this one ThreadLocal read.
      final boolean timed = Timing.active();
      final long bracketed = timed ? Timing.total("moduleBody") + Timing.total("harvest") : 0L;
      try {
        completion = script.exec(cx, scope);
      } catch (RhinoException e) {
        return errorFromRhino(id, "runtime_error", e);
      } catch (Error e) {
        if (isInterrupt(e)) {
          return timeoutResponse(id);
        }
        throw e;
      } finally {
        Timing.add("subjectExec", phase);
        if (timed) {
          // Closes any bracket the script left open, so the subtraction sees it.
          Timing.exit(0);
          Timing.addLess("subjectOwn", phase, Timing.total("moduleBody") + Timing.total("harvest") - bracketed);
        }
      }
      // Harvest outside the author try: its failures are emulation failures, not script errors.
      if (bridge != null) {
        phase = Timing.now();
        try { return response(id, "ok", JsValues.of(bridge.harvest(cx)), null); }
        finally { Timing.add("harvest", phase); }
      }
      // A context whose result is the script's completion value (OIDC claims:
      // AM reads the value the script evaluates to) names a preamble function
      // that receives it, so the author source runs unmodified and the value
      // is exactly what the script left. Its failures are the script's, as
      // the call happens before the response, and are reported like them.
      final String completionHarvest = Json.optionalString(job, "completionHarvest");
      if (completionHarvest != null) {
        final Object harvest = ScriptableObject.getProperty(scope, completionHarvest);
        if (!(harvest instanceof org.mozilla.javascript.Function)) {
          throw new IllegalArgumentException("completionHarvest " + completionHarvest + " is not a function");
        }
        phase = Timing.now();
        try {
          final Object harvested = ((org.mozilla.javascript.Function) harvest).call(cx, scope, scope, new Object[] { completion });
          return response(id, "ok", JsValues.of(harvested), null);
        } catch (RhinoException e) {
          return errorFromRhino(id, "runtime_error", e);
        } finally {
          Timing.add("harvest", phase);
        }
      }
      final String resultGlobal = Json.optionalString(job, "resultGlobal");
      final Object result = resultGlobal == null
          ? completion
          : ScriptableObject.getProperty(scope, resultGlobal);
      return response(id, "ok", JsValues.of(result), null);
    } finally {
      Context.exit();
    }
  }

  /** Defined for the preamble only, like {@code __rhinoLocalHostOp}. */
  static final String LIBRARIES_NAME = "__rhinoLocalLibraries";
  static final String PREAMBLE_DATA_NAME = "__rhinoLocalPreambleData";

  private static Map<String, String> stringMap(Object raw, String what) {
    if (raw == null) return null;
    Map<String, String> out = new LinkedHashMap<String, String>();
    for (Map.Entry<String, Object> e : Json.asObject(raw, what).entrySet()) {
      if (!(e.getValue() instanceof String)) throw new IllegalArgumentException(what + "." + e.getKey() + " must be a string");
      out.put(e.getKey(), (String) e.getValue());
    }
    return out;
  }

  static void injectGlobals(SimpleScriptContext scriptContext, Object globalsRaw) {
    if (globalsRaw == null) {
      return;
    }
    Map<String, Object> globals = Json.asObject(globalsRaw, "globals");
    for (Map.Entry<String, Object> e : globals.entrySet()) {
      scriptContext.setAttribute(e.getKey(), e.getValue(), ScriptContext.ENGINE_SCOPE);
    }
  }

  static boolean isInterrupt(Error e) {
    return "Interrupt.".equals(e.getMessage());
  }

  static Map<String, Object> errorFromRhino(String id, String outcome, RhinoException e) {
    return errorResponse(
        id, outcome, e, e.sourceName(), e.lineNumber(), e.columnNumber(), e.lineSource());
  }

  static Map<String, Object> timeoutResponse(String id) {
    Map<String, Object> err = new LinkedHashMap<String, Object>();
    err.put("class", "java.lang.Error");
    err.put("message", "Interrupt.");
    err.put("sourceName", null);
    err.put("line", Integer.valueOf(-1));
    err.put("column", Integer.valueOf(-1));
    err.put("lineSource", null);
    return response(id, "timeout", JsValues.of(Scriptable.NOT_FOUND), err);
  }

  static Map<String, Object> errorResponse(
      String id,
      String outcome,
      Throwable t,
      String sourceName,
      int line,
      int column,
      String lineSource) {
    Map<String, Object> err = new LinkedHashMap<String, Object>();
    err.put("class", t.getClass().getName());
    // An exception with no text of its own (a write to a list that takes none) is named by class.
    err.put("message", t.getMessage() != null ? t.getMessage() : t.getClass().getName());
    err.put("sourceName", sourceName);
    err.put("line", Integer.valueOf(line));
    err.put("column", Integer.valueOf(column));
    err.put("lineSource", lineSource);
    return response(id, outcome, JsValues.of(Scriptable.NOT_FOUND), err);
  }

  static Map<String, Object> response(
      String id, String outcome, JsValues value, Map<String, Object> error) {
    Map<String, Object> rec = new LinkedHashMap<String, Object>();
    rec.put("id", id);
    rec.put("outcome", outcome);
    rec.put("valueKind", value.kind);
    rec.put("value", value.json);
    rec.put("error", error);
    return rec;
  }

  static synchronized void write(Map<String, Object> rec) {
    OUT.println(Json.stringify(ProtocolValues.encode(rec)));
  }
}
