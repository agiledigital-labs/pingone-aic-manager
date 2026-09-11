import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import javax.script.SimpleScriptContext;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.RhinoException;
import org.mozilla.javascript.Script;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;

/**
 * Evaluate one JavaScript source file under a Rhino Context and scope shaped
 * like AM 8.1.1's decision-node eval: {@code ObservedJavaScriptContext} knobs
 * plus {@code ScriptContextScope} over JSR-223 Bindings with {@code
 * initStandardObjects()} as the prototype.
 *
 * <p>Prints one JSON object on stdout. The well-known global {@code __result}
 * is how a corpus script reports a value: that is what distinguishes "parses
 * but the binding is undefined" from "parses and works".
 *
 * <pre>
 * java -cp rhino.jar:classes Probe &lt;languageVersion&gt; &lt;source.js&gt;
 * </pre>
 *
 * {@code languageVersion} is a Rhino {@code Context} version integer, or one
 * of {@code DEFAULT}, {@code 1.7}, {@code 1.8}, {@code ES6}.
 */
public final class Probe {

  static final String RESULT_GLOBAL = "__result";

  public static void main(String[] args) throws Exception {
    if (args.length != 2) {
      System.err.println("usage: Probe <languageVersion> <source.js>");
      System.exit(2);
    }
    final int languageVersion = LanguageVersions.parse(args[0]);
    final Path sourcePath = Path.of(args[1]);
    final String source = Files.readString(sourcePath, StandardCharsets.UTF_8);
    final String sourceName = sourcePath.getFileName().toString();

    final AmContextFactory factory = new AmContextFactory();
    final Context cx = factory.enterContext();
    try {
      cx.setLanguageVersion(languageVersion);
      final SimpleScriptContext scriptContext = AmScopes.newEngineContext(sourceName);
      final Scriptable scope = AmScopes.makeScriptable(cx, scriptContext);

      boolean compiled = false;
      boolean evaluated = false;
      String exceptionClass = null;
      String exceptionMessage = null;

      try {
        Script script = cx.compileString(source, sourceName, 1, null);
        compiled = true;
        try {
          script.exec(cx, scope);
          evaluated = true;
        } catch (RhinoException e) {
          exceptionClass = e.getClass().getName();
          exceptionMessage = e.getMessage();
        }
      } catch (RhinoException e) {
        exceptionClass = e.getClass().getName();
        exceptionMessage = e.getMessage();
      }

      final Object raw =
          evaluated ? ScriptableObject.getProperty(scope, RESULT_GLOBAL) : Scriptable.NOT_FOUND;
      final JsValues result = JsValues.of(raw);

      final StringBuilder json = new StringBuilder();
      json.append('{');
      field(json, "implementationVersion", cx.getImplementationVersion(), true);
      json.append(',');
      json.append("\"languageVersion\":").append(languageVersion).append(',');
      field(json, "source", sourceName, true);
      json.append(',');
      json.append("\"compiled\":").append(compiled).append(',');
      json.append("\"evaluated\":").append(evaluated).append(',');
      field(json, "exceptionClass", exceptionClass, true);
      json.append(',');
      field(json, "exceptionMessage", exceptionMessage, true);
      json.append(',');
      json.append("\"resultDefined\":").append(result.defined).append(',');
      field(json, "resultKind", result.kind, true);
      json.append(',');
      json.append("\"result\":").append(Json.stringify(result.json));
      json.append('}');
      System.out.println(json);
    } finally {
      Context.exit();
    }
  }

  private static void field(StringBuilder json, String name, String value, boolean quoted) {
    json.append('"').append(name).append("\":");
    if (value == null) {
      json.append("null");
    } else if (quoted) {
      json.append(Json.quote(value));
    } else {
      json.append(value);
    }
  }
}
