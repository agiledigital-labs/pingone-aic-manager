import java.util.HashSet;
import java.util.List;
import java.util.Set;
import javax.script.SimpleScriptContext;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.NativeObject;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;
import org.mozilla.javascript.Wrapper;
import org.mozilla.javascript.json.JsonParser;

/**
 * The scope a Node Designer (custom node) script runs in: its own top-level scope, not the
 * Scripted Decision scope the preamble filled.
 *
 * <pre>
 *   custom scope (ScriptContextScope, no parent): properties, nodeState, outcome
 *     prototype: {@link Lookup}, which refuses a read of a blocked binding by name
 *       prototype: the job's standard objects (the decision scope's own prototype)
 * </pre>
 *
 * <p>The author source runs unmodified as a top-level script, so Rhino hoists its declarations,
 * reports its own line numbers, and gives every name it declares or assigns an ordinary slot in
 * this scope. The harness's {@code action} and every other decision binding live in the other
 * scope, out of the script's reach: nothing it declares can shadow the routing, which the host
 * reads from {@code outcome} after the script returns, and nothing it declares is harvested as
 * the decision scope's {@code auditEntryDetail}.
 */
final class CustomNodeScope {

  /** The restricted nodeState view the journey preamble builds in the decision scope. */
  static final String NODE_STATE = "__rhinoLocalCustomNodeState";

  private CustomNodeScope() {}

  static Scriptable create(
      Context cx, Scriptable decision, AmHostBridge bridge, List<String> blocked, String properties) {
    Object nodeState = ScriptableObject.getProperty(decision, NODE_STATE);
    if (!(nodeState instanceof Scriptable)) throw bridge.protocolFailure("custom node nodeState view");
    ScriptContextScope scope = new ScriptContextScope(new SimpleScriptContext());
    scope.setPrototype(new Lookup(bridge, new HashSet<String>(blocked), decision.getPrototype()));
    Object parsed;
    try {
      parsed = new JsonParser(cx, decision).parseValue(properties);
    } catch (JsonParser.ParseException e) {
      throw bridge.protocolFailure("custom node properties: " + e.getMessage());
    }
    scope.put("properties", scope, parsed);
    scope.put("nodeState", scope, nodeState);
    scope.put("outcome", scope, Undefined.instance);
    return scope;
  }

  /**
   * The one route decision for a custom node, after it returned or threw: a listed {@code outcome}
   * is itself; anything else, and every throw ({@code outcome} null), takes {@code errorRoute}
   * (Script Error, absent when the type has none), else null -- no route, and the journey fails.
   */
  static String route(String outcome, List<String> listed, String errorRoute) {
    if (outcome != null && listed.contains(outcome)) return outcome;
    return errorRoute;
  }

  /**
   * {@code outcome} as the script left it: a string, or null for anything else or nothing. A
   * computed string ({@code 'o' + suffix}) is a Rhino ConsString, which this scope hands back
   * wrapped as a Java object (its read goes through {@code Context.javaToJS}); it is unwrapped
   * first, so it routes as the string it is. A {@code new String('ok')} object is not a string
   * primitive and stays null.
   */
  static String outcome(Scriptable scope) {
    Object value = ScriptableObject.getProperty(scope, "outcome");
    if (value instanceof Wrapper) value = ((Wrapper) value).unwrap();
    return value instanceof CharSequence ? value.toString() : null;
  }

  /**
   * Answers a <em>read</em> of a blocked binding with its named refusal, and is otherwise empty.
   *
   * <p>It never says it has a blocked name ({@link #has} is false), because Rhino asks {@code has}
   * along the prototype chain when it declares a top-level {@code var} or function and when it
   * binds an unqualified assignment: a yes there would send the script's own variable to this
   * object instead of giving it a slot of its own. A read ({@link #get}) is where the refusal
   * lives: a free {@code logger} and {@code this.logger} both reach it, and only when the script
   * has no variable of that name, since its own slot is found first.
   *
   * <p>A bare {@code typeof name} is the one read that goes through {@code has} instead of
   * {@code get} (Rhino binds the name first), and {@code has} cannot tell which operator asked, so
   * a bare {@code typeof} of a blocked name the script never bound reads {@code "undefined"}, as
   * for any unbound name. {@code typeof this.name} is a property read and gets the refusal.
   * {@code "name" in this} also asks {@code has}, and reads {@code false}.
   */
  static final class Lookup implements Scriptable {
    private final AmHostBridge bridge;
    private final Set<String> blocked;
    private Scriptable prototype;

    Lookup(AmHostBridge bridge, Set<String> blocked, Scriptable prototype) {
      this.bridge = bridge;
      this.blocked = blocked;
      this.prototype = prototype;
    }

    private RuntimeException refuse(String name) {
      throw bridge.refusal("custom node binding: " + name);
    }

    @Override
    public String getClassName() {
      return "Object";
    }

    @Override
    public Object get(String name, Scriptable start) {
      if (blocked.contains(name)) throw refuse(name);
      return Scriptable.NOT_FOUND;
    }

    @Override
    public Object get(int index, Scriptable start) {
      return Scriptable.NOT_FOUND;
    }

    @Override
    public boolean has(String name, Scriptable start) {
      return false;
    }

    @Override
    public boolean has(int index, Scriptable start) {
      return false;
    }

    @Override
    public void put(String name, Scriptable start, Object value) {
      if (start != this) start.put(name, start, value);
    }

    @Override
    public void put(int index, Scriptable start, Object value) {
      if (start != this) start.put(index, start, value);
    }

    @Override
    public void delete(String name) {}

    @Override
    public void delete(int index) {}

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
      return null;
    }

    @Override
    public void setParentScope(Scriptable parent) {}

    @Override
    public Object[] getIds() {
      return new Object[0];
    }

    @Override
    public Object getDefaultValue(Class<?> hint) {
      return NativeObject.getDefaultValue(this, hint);
    }

    @Override
    public boolean hasInstance(Scriptable instance) {
      return false;
    }
  }
}
