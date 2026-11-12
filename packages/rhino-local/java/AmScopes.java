import javax.script.ScriptContext;
import javax.script.SimpleScriptContext;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;

/**
 * The scope a script runs in.
 *
 * <pre>
 *   ScriptContextScope scope = new ScriptContextScope(scriptContext);
 *   ScriptableObject std = cx.initStandardObjects();
 *   scope.setPrototype(std);
 *   scope.put("context", scope, scriptContext);
 *   return scope;
 * </pre>
 *
 * <p>The standard-objects object is the <em>prototype</em>, not the parent
 * (parent stays {@code null}). A CommonJS {@code require} is not installed
 * here (decision-node eval).
 *
 * <p>{@code initStandardObjects()} runs on every eval — no sealed
 * standard-objects parent is shared — so {@code Object.prototype} mutations
 * cannot leak from job N to job N+1.
 */
final class AmScopes {

  static final String FILENAME_KEY = "javax.script.filename";

  private AmScopes() {}

  static Scriptable makeScriptable(Context cx, ScriptContext scriptContext) {
    ScriptContextScope scope = new ScriptContextScope(scriptContext);
    ScriptableObject std = cx.initStandardObjects();
    scope.setPrototype(std);
    scope.put("context", scope, scriptContext);
    return scope;
  }

  static SimpleScriptContext newEngineContext(String sourceName) {
    SimpleScriptContext scriptContext = new SimpleScriptContext();
    if (sourceName != null) {
      scriptContext.setAttribute(FILENAME_KEY, sourceName, ScriptContext.ENGINE_SCOPE);
    }
    return scriptContext;
  }
}
