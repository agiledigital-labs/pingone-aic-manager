import org.mozilla.javascript.Context;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Function;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.tools.shell.Global;

/** The shell global's prototype resolves named bindings but cannot enumerate them. */
final class IdmScope extends ScriptableObject {
  @Override public String getClassName() { return "ScriptableWithDeferredBinding"; }
  @Override public Object[] getIds() {
    throw new UnsupportedOperationException("There is no way know all the bound ids in a deferred binding scope");
  }

  static Global create(Context cx, IdmContextFactory factory) {
    // Measured warm initialization: ~0.063 ms. Fresh globals keep prototype mutations job-local.
    Global global = new Global(cx);
    Function shellSpawn = (Function) ScriptableObject.getProperty(global, "spawn");
    ScriptableObject.putProperty(global, "spawn", new BaseFunction(global, ScriptableObject.getFunctionPrototype(global)) {
      @Override public String getFunctionName() { return "spawn"; }
      @Override public int getLength() { return ((BaseFunction) shellSpawn).getLength(); }
      @Override public Object call(Context child, Scriptable s, Scriptable self, Object[] args) {
        return factory.spawn(child, global, args, shellSpawn);
      }
    });
    // Shell print must never corrupt the JSON line protocol.
    global.setOut(Runner.ERR);
    global.setErr(Runner.ERR);
    IdmScope deferred = new IdmScope();
    deferred.setPrototype(global.getPrototype());
    deferred.setParentScope(global);
    global.setPrototype(deferred);
    return global;
  }
}
