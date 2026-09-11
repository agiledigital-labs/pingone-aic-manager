import javax.script.ScriptContext;
import javax.script.SimpleScriptContext;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;

/**
 * AM 8.1.1 {@code RhinoScriptEngine.makeScriptable} / {@code getScope},
 * measured with {@code javap -p -c}.
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
 * (parent stays {@code null}). {@code getScope} additionally installs CommonJS
 * {@code require} when {@code LIBRARY_SCRIPT} is on and {@code libraryBindings}
 * is present; that path is not modelled here (decision-node eval).
 *
 * <p>AM calls {@code initStandardObjects()} on every eval — it does not share a
 * sealed standard-objects parent. We do the same, so {@code Object.prototype}
 * mutations cannot leak from job N to job N+1.
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
