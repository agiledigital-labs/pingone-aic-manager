import org.mozilla.javascript.Context;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.Undefined;

/** Classify a Rhino completion / binding value for JSON emission. */
final class JsValues {

  final boolean defined;
  final String kind;
  final Object json;

  private JsValues(boolean defined, String kind, Object json) {
    this.defined = defined;
    this.kind = kind;
    this.json = json;
  }

  static JsValues of(Object raw) {
    if (raw == Scriptable.NOT_FOUND) {
      return new JsValues(false, "missing", null);
    }
    if (raw == null) {
      return new JsValues(true, "null", null);
    }
    if (Undefined.isUndefined(raw)) {
      return new JsValues(true, "undefined", null);
    }
    if (raw instanceof Boolean) {
      return new JsValues(true, "boolean", raw);
    }
    if (raw instanceof Number) {
      return new JsValues(true, "number", raw);
    }
    if (raw instanceof String) {
      return new JsValues(true, "string", raw);
    }
    return new JsValues(true, "string", Context.toString(raw));
  }
}
