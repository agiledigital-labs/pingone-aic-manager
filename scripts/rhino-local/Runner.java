import java.io.BufferedReader;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.Map;
import javax.script.ScriptContext;
import javax.script.SimpleScriptContext;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.EvaluatorException;
import org.mozilla.javascript.RhinoException;
import org.mozilla.javascript.Script;
import org.mozilla.javascript.Scriptable;

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

  public static void main(String[] args) throws Exception {
    final AmContextFactory factory = new AmContextFactory();
    ERR.println("rhino-local-runner ready");
    final BufferedReader in =
        new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
    String line;
    while ((line = in.readLine()) != null) {
      if (line.isEmpty()) {
        continue;
      }
      handleLine(factory, line);
    }
  }

  static void handleLine(AmContextFactory factory, String line) {
    final Object parsed;
    try {
      parsed = Json.parse(line);
    } catch (RuntimeException e) {
      ERR.println("rhino-local-runner: skipping unparseable line: " + e.getMessage());
      return;
    }
    final Map<String, Object> job;
    try {
      job = Json.asObject(parsed, "job");
    } catch (RuntimeException e) {
      ERR.println("rhino-local-runner: skipping non-object job: " + e.getMessage());
      return;
    }
    final Object idRaw = job.get("id");
    if (!(idRaw instanceof String) || ((String) idRaw).isEmpty()) {
      ERR.println("rhino-local-runner: skipping job with no string id");
      return;
    }
    final String id = (String) idRaw;
    try {
      if (Json.optionalBoolean(job, "shutdown", false)
          || "shutdown".equals(Json.optionalString(job, "op"))) {
        write(response(id, "ok", JsValues.of(Scriptable.NOT_FOUND), null));
        OUT.flush();
        System.exit(0);
        return;
      }
      write(evalJob(factory, id, job));
    } catch (Throwable t) {
      write(errorResponse(id, "runtime_error", t, null, -1, -1, null));
    }
  }

  static Map<String, Object> evalJob(
      AmContextFactory factory, String id, Map<String, Object> job) {
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

    final Context cx = factory.enterContext();
    try {
      cx.setLanguageVersion(languageVersion);
      if (cx instanceof AmContextFactory.ObservedContext) {
        ((AmContextFactory.ObservedContext) cx).timeoutMs = timeoutMs;
      }
      final SimpleScriptContext scriptContext = AmScopes.newEngineContext(sourceName);
      injectGlobals(scriptContext, job.get("globals"));
      final Scriptable scope = AmScopes.makeScriptable(cx, scriptContext);

      if (preamble != null && !preamble.isEmpty()) {
        try {
          Script preambleScript = cx.compileString(preamble, preambleName, 1, null);
          preambleScript.exec(cx, scope);
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
      try {
        script = cx.compileString(source, sourceName, 1, null);
      } catch (EvaluatorException e) {
        return errorFromRhino(id, "compile_error", e);
      } catch (RhinoException e) {
        return errorFromRhino(id, "compile_error", e);
      }

      final Object completion;
      try {
        completion = script.exec(cx, scope);
      } catch (RhinoException e) {
        return errorFromRhino(id, "runtime_error", e);
      } catch (Error e) {
        if (isInterrupt(e)) {
          return timeoutResponse(id);
        }
        throw e;
      }
      return response(id, "ok", JsValues.of(completion), null);
    } finally {
      Context.exit();
    }
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
    err.put("message", t.getMessage());
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

  static void write(Map<String, Object> rec) {
    OUT.println(Json.stringify(rec));
  }
}
