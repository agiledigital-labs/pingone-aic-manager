import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Function;
import org.mozilla.javascript.ScriptRuntime;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;

/**
 * A builtin's replacement that carries the builtin's own metadata: its
 * {@code name}, its {@code length} (and Rhino's {@code arity}), no own
 * {@code prototype}, and no constructor behaviour. A JavaScript function
 * cannot shed its {@code prototype} (it is non-configurable), so the runtime's
 * wrappers of {@code Function.prototype.bind} and {@code toString} are built
 * here, with the runtime's function as the body.
 *
 * <p>{@link Factory} is defined for the prelude only, as
 * {@code __rhinoLocalNativeMethodHost(name, length, body)}.
 */
final class NativeMethod extends BaseFunction {
  static final String FACTORY_NAME = "__rhinoLocalNativeMethodHost";

  private final String name;
  private final int length;
  private final Function body;

  private NativeMethod(String name, int length, Function body) {
    this.name = name;
    this.length = length;
    this.body = body;
  }

  @Override
  public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
    return body.call(cx, scope, thisObj, args);
  }

  @Override
  public Scriptable construct(Context cx, Scriptable scope, Object[] args) {
    throw ScriptRuntime.typeErrorById("msg.not.ctor", name);
  }

  @Override
  public String getFunctionName() {
    return name;
  }

  @Override
  public int getArity() {
    return length;
  }

  @Override
  public int getLength() {
    return length;
  }

  @Override
  protected boolean hasPrototypeProperty() {
    return false;
  }

  /** {@code (name, length, body)} to a {@link NativeMethod}. */
  static final class Factory extends BaseFunction {
    @Override
    public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
      NativeMethod method = new NativeMethod(
          Context.toString(args[0]), (int) Context.toNumber(args[1]), (Function) args[2]);
      Scriptable top = ScriptableObject.getTopLevelScope(scope);
      method.setParentScope(top);
      method.setPrototype(ScriptableObject.getFunctionPrototype(top));
      return method;
    }
  }
}
