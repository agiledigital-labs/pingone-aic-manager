import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
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
 * a {@link LibraryScope} over the calling job's scope from a compiled form
 * shared by every job whose library has the same source. The runtime still
 * owns the require cache, cycles and the undeclared-library refusal.
 *
 * <p>{@code host(id, source)} compiles a library the job seeded directly
 * (a runner caller's {@code given.libraries}) into the same library scope,
 * uncached, so no path on the JVM evaluates a library in its caller's scope.
 */
final class LibraryHost extends BaseFunction {

  /** The wrapper the runtime's eval path uses, so library line numbers do not move. */
  static final String PREFIX = "(function (require, exports, module) {\n";
  static final String SUFFIX = "\n})";

  /** The runtime's copy of the measured unbound list (runtime.cjs). */
  static final String UNBOUND_NAME = "__rhinoLocalLibUnbound";

  private final Map<String, String> refs;
  private Set<String> visible = Collections.emptySet();

  /** {@code refs}: require() id to source hash, each hash already registered. */
  LibraryHost(Map<String, String> refs) {
    this.refs = refs;
  }

  /**
   * After the preamble, before the script: what a library may see is what was
   * bound then, less the measured unbound list. Names the script declares
   * afterwards (a caller's local {@code var}) are therefore not visible.
   */
  void snapshot(Scriptable scope) {
    Set<String> names = new HashSet<String>();
    for (Object id : scope.getIds()) {
      if (id instanceof String) names.add((String) id);
    }
    Object unbound = ScriptableObject.getProperty(scope, UNBOUND_NAME);
    if (unbound instanceof List) {
      for (Object name : (List<?>) unbound) names.remove(Context.toString(name));
    }
    visible = Collections.unmodifiableSet(names);
  }

  @Override
  public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
    String id = args.length > 0 ? Context.toString(args[0]) : "undefined";
    String hash = refs.get(id);
    String seeded = args.length > 1 && args[1] instanceof CharSequence ? args[1].toString() : null;
    if (hash == null && seeded == null) return Undefined.instance;
    Script loader;
    Timing.count("libraryLoads");
    long phase = Timing.now();
    try {
      // Line 0: PREFIX ends in a newline, so the body's first line is reported as line 1.
      if (hash != null) {
        ScriptCache.Source source = ScriptCache.source(hash);
        loader = ScriptCache.compile(cx, "library:" + hash, () -> PREFIX + source.text + SUFFIX, id, 0);
      } else {
        loader = cx.compileString(PREFIX + seeded + SUFFIX, id, 0, null);
      }
    } catch (EvaluatorException e) {
      // What the runtime's eval path threw before the cache: a catchable SyntaxError.
      throw ScriptRuntime.constructError("SyntaxError", e.details(), e.sourceName(), e.lineNumber(),
          e.lineSource(), e.columnNumber());
    } finally {
      Timing.add("libraryCompile", phase);
    }
    phase = Timing.now();
    try {
      return loader.exec(cx, new LibraryScope(ScriptableObject.getTopLevelScope(this), visible));
    } finally {
      Timing.add("libraryLoaderExec", phase);
    }
  }
}
