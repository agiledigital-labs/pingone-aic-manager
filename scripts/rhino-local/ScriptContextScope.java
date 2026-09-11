import java.util.ArrayList;
import java.util.List;
import javax.script.Bindings;
import javax.script.ScriptContext;
import org.mozilla.javascript.Context;
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

  @Override
  public Object get(String name, Scriptable start) {
    int scope = scriptContext.getAttributesScope(name);
    if (scope != -1) {
      Object value = scriptContext.getAttribute(name, scope);
      return Context.javaToJS(value, this);
    }
    return Scriptable.NOT_FOUND;
  }

  @Override
  public Object get(int index, Scriptable start) {
    return Scriptable.NOT_FOUND;
  }

  @Override
  public void put(String name, Scriptable start, Object value) {
    int scope = scriptContext.getAttributesScope(name);
    if (scope == -1) {
      scope = ScriptContext.ENGINE_SCOPE;
    }
    if (value instanceof Wrapper) {
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
