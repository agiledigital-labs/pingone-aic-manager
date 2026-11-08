import java.util.Map;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.EvaluatorException;
import org.mozilla.javascript.Script;
import org.mozilla.javascript.ScriptRuntime;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;

/**
 * The job's library map, as the runtime's {@code require} reads it.
 *
 * <p>Like {@link HostOps} it is defined for the preamble only, captured by the
 * runtime and deleted before the author's script runs. {@code host(id)} answers
 * {@code undefined} for an undeclared id, else the module's loader function
 * {@code (function (require, exports, module) { … })}, freshly evaluated in
 * the calling job's scope from a compiled form shared by every job whose
 * library has the same source. The runtime still owns the require cache,
 * cycles and the undeclared-library refusal; this only replaces the
 * {@code eval} that used to recompile each body in every case.
 */
final class LibraryHost extends BaseFunction {

  /** The wrapper the runtime's eval path uses, so library line numbers do not move. */
  static final String PREFIX = "(function (require, exports, module) {\n";
  static final String SUFFIX = "\n})";

  private final Map<String, String> refs;

  /** {@code refs}: require() id to source hash, each hash already registered. */
  LibraryHost(Map<String, String> refs) {
    this.refs = refs;
  }

  @Override
  public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
    String id = args.length > 0 ? Context.toString(args[0]) : "undefined";
    String hash = refs.get(id);
    if (hash == null) return Undefined.instance;
    ScriptCache.Source source = ScriptCache.source(hash);
    Script loader;
    Timing.count("libraryLoads");
    long phase = Timing.now();
    try {
      // Line 0: PREFIX ends in a newline, so the body's first line is reported as line 1.
      loader = ScriptCache.compile(cx, "library:" + hash, () -> PREFIX + source.text + SUFFIX, id, 0);
    } catch (EvaluatorException e) {
      // What the runtime's eval path threw before the cache: a catchable SyntaxError.
      throw ScriptRuntime.constructError("SyntaxError", e.details(), e.sourceName(), e.lineNumber(),
          e.lineSource(), e.columnNumber());
    } finally {
      Timing.add("libraryCompile", phase);
    }
    phase = Timing.now();
    try {
      return loader.exec(cx, ScriptableObject.getTopLevelScope(this));
    } finally {
      Timing.add("libraryLoaderExec", phase);
    }
  }
}
