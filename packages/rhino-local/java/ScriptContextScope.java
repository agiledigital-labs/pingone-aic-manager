import java.util.ArrayList;
import java.util.List;
import javax.script.Bindings;
import javax.script.ScriptContext;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.JavaScriptException;
import org.mozilla.javascript.NativeObject;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.Wrapper;

/**
 * Replica of AM 8.1.1 {@code
 * org.forgerock.openam.scripting.factories.ScriptContextScope}, measured with
 * {@code javap -p -c} on {@code openam-scripting-8.1.1.jar}.
 *
 * <p>Implements {@link Scriptable} only — not {@code ScriptableObject} and not
 * {@code ConstProperties}. That is the lever for top-level {@code const}:
 * Rhino's {@code putConstProperty} silently drops the initializer when the
 * scope is not {@code ConstProperties}, after {@code defineConstProperty} has
 * already {@code put} {@code Undefined}. Decision-node scripts therefore read
 * top-level {@code const} back as {@code undefined}.
 *
 * <p>AM's class depends on {@code org.forgerock.util.Reject}; this replica does
 * not, so we do not have to extract forgerock-util.
 */
final class ScriptContextScope implements Scriptable {

  private final ScriptContext scriptContext;
  private Scriptable prototype;
  private Scriptable parentScope;

  ScriptContextScope(ScriptContext scriptContext) {
    if (scriptContext == null) {
      throw new IllegalArgumentException("scriptContext");
    }
    this.scriptContext = scriptContext;
  }

  @Override
  public String getClassName() {
    return "ScriptContextScope";
  }

  @Override
  public boolean has(String name, Scriptable start) {
    return scriptContext.getAttributesScope(name) != -1;
  }

  @Override
  public boolean has(int index, Scriptable start) {
    return false;
  }

  /**
   * Set by the may-act seed on the legacy engine, which runs without a class shutter so that
   * {@code java.util} stays visible. Reading {@code context} still has to throw the shutter's
   * refusal of {@code javax.script.SimpleScriptContext}: that is what {@code typeof context} does
   * on AIC, and a getter cannot be installed on this scope (it is not a {@code ScriptableObject}).
   */
  private static final String THROW_CONTEXT = "__rhinoLocalThrowContext";

  private void refuseContext() {
    Context cx = Context.getCurrentContext();
    Scriptable error =
        cx.newObject(
            this,
            "InternalError",
            new Object[] {"Access to Java class \"javax.script.SimpleScriptContext\" is prohibited."});
    throw new JavaScriptException(error, null, 0);
  }

  @Override
  public Object get(String name, Scriptable start) {
    int scope = scriptContext.getAttributesScope(name);
    if (scope != -1) {
      if ("context".equals(name) && scriptContext.getAttributesScope(THROW_CONTEXT) != -1) {
        refuseContext();
      }
      Object value = scriptContext.getAttribute(name, scope);
      return Context.javaToJS(value, this);
    }
    return Scriptable.NOT_FOUND;
  }

  @Override
  public Object get(int index, Scriptable start) {
    return Scriptable.NOT_FOUND;
  }

  private java.util.Set<String> sealedNames = java.util.Collections.emptySet();

  /**
   * After the preamble: the harness's own globals -- every {@code __rhinoLocal…} name defined by
   * then -- become read-only and permanent to the script, as a non-writable property is in sloppy
   * mode: an assignment or delete is ignored. A script that rebinds a harness helper must not be
   * able to turn a harness fault into an ordinary error it catches. Names a script declares later
   * (the AIC-lane wrapper's own {@code __rhinoLocalOutcome}) stay ordinary variables.
   */
  void sealHarnessGlobals() {
    java.util.Set<String> names = new java.util.HashSet<>();
    for (int scope : scriptContext.getScopes()) {
      javax.script.Bindings bindings = scriptContext.getBindings(scope);
      if (bindings == null) {
        continue;
      }
      for (String name : bindings.keySet()) {
        if (name.startsWith("__rhinoLocal")) {
          names.add(name);
        }
      }
    }
    sealedNames = names;
  }

  private boolean sealed(String name) {
    return sealedNames.contains(name);
  }

  @Override
  public void put(String name, Scriptable start, Object value) {
    if (sealed(name)) {
      return;
    }
    int scope = scriptContext.getAttributesScope(name);
    if (scope == -1) {
      scope = ScriptContext.ENGINE_SCOPE;
    }
    // A guarded map (ContextJavaGuard, Sol ctx r2 P2-6) is stored as the guard
    // itself: AM stores the bare map and wraps it afresh on every read, which
    // here would hand the script an unguarded NativeJavaMap from the global
    // `claims`. Either way the script reads the same map. A guarded String is
    // still unwrapped, because a top-level var flattening a Java String to a
    // JS string is measured.
    if (value instanceof Wrapper && !(value instanceof ContextJavaGuard.GuardedMap)) {
      value = ((Wrapper) value).unwrap();
    }
    scriptContext.setAttribute(name, value, scope);
  }

  @Override
  public void put(int index, Scriptable start, Object value) {
    // AM no-ops integer keys.
  }

  @Override
  public void delete(String name) {
    if (sealed(name)) {
      return;
    }
    int scope = scriptContext.getAttributesScope(name);
    if (scope != -1) {
      scriptContext.removeAttribute(name, scope);
    }
  }

  @Override
  public void delete(int index) {
    // AM no-ops integer keys.
  }

  @Override
  public Scriptable getPrototype() {
    return prototype;
  }

  @Override
  public void setPrototype(Scriptable prototype) {
    this.prototype = prototype;
  }

  @Override
  public Scriptable getParentScope() {
    return parentScope;
  }

  @Override
  public void setParentScope(Scriptable parentScope) {
    this.parentScope = parentScope;
  }

  @Override
  public Object[] getIds() {
    List<Object> ids = new ArrayList<Object>();
    for (Integer scope : scriptContext.getScopes()) {
      Bindings bindings = scriptContext.getBindings(scope.intValue());
      // AM does not null-check; SimpleScriptContext's GLOBAL_SCOPE starts null.
      if (bindings != null) {
        ids.addAll(bindings.keySet());
      }
    }
    return ids.toArray();
  }

  @Override
  public Object getDefaultValue(Class<?> hint) {
    return NativeObject.getDefaultValue(this, hint);
  }

  @Override
  public boolean hasInstance(Scriptable instance) {
    Scriptable proto = instance.getPrototype();
    while (proto != null) {
      if (proto.equals(this)) {
        return true;
      }
      proto = proto.getPrototype();
    }
    return false;
  }
}
