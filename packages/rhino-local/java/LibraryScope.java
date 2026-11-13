import java.util.Set;
import org.mozilla.javascript.NativeObject;
import org.mozilla.javascript.Scriptable;

/**
 * The scope a {@code require()}d library's loader runs in, and so the scope its
 * exports close over.
 *
 * <p>On AIC a required library does not see its caller's scope (docs/api/12,
 * "Library scope when required", measured 2026-11-11): the caller's context
 * bindings ({@code nodeState}, {@code action}, {@code accessToken}, …) are
 * unbound inside it, and so is a local {@code var} the caller declared.
 * Reading one is the engine's own {@code ReferenceError}, and {@code typeof}
 * of one is {@code "undefined"}, because a name this scope does not answer
 * falls through to the standard objects and then to nothing.
 *
 * <p>What it does answer: the names bound when the preamble finished
 * ({@code visible}: the shared bindings, {@code require}, {@code scriptName},
 * the harness's own internals) less the measured unbound list, read live from
 * the caller's scope. A name the caller's script declares later is not in
 * {@code visible}. An assignment the library makes to an undeclared name lands
 * here, not in the caller's scope; whether AIC shares those is unmeasured.
 */
final class LibraryScope extends NativeObject {

  private static final long serialVersionUID = 1L;

  private final Scriptable caller;
  private final Set<String> visible;

  LibraryScope(Scriptable caller, Set<String> visible) {
    this.caller = caller;
    this.visible = visible;
    setPrototype(caller.getPrototype());
    setParentScope(null);
  }

  private boolean delegated(String name) {
    return visible.contains(name) && caller.has(name, caller);
  }

  @Override
  public String getClassName() {
    return "global";
  }

  @Override
  public boolean has(String name, Scriptable start) {
    return super.has(name, start) || delegated(name);
  }

  @Override
  public Object get(String name, Scriptable start) {
    if (super.has(name, start)) {
      return super.get(name, start);
    }
    return delegated(name) ? caller.get(name, caller) : Scriptable.NOT_FOUND;
  }
}
