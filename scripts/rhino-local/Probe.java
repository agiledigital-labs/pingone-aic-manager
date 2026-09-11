import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.ContextFactory;
import org.mozilla.javascript.RhinoException;
import org.mozilla.javascript.Script;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;

/**
 * Evaluate one JavaScript source file under a Rhino Context configured like AM
 * 8.1.1's {@code ObservedJavaScriptContext}: interpreted ({@code
 * setOptimizationLevel(-1)}), instruction-observer threshold 1000, max
 * interpreter stack depth 10000, and {@code FEATURE_ENABLE_JAVA_MAP_ACCESS}
 * (Rhino feature 21) on. Language version is selected on the command line so
 * the same corpus can run under {@code VERSION_DEFAULT} (0) and {@code
 * VERSION_ES6} (200).
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
    final int languageVersion = parseLanguageVersion(args[0]);
    final Path sourcePath = Path.of(args[1]);
    final String source = Files.readString(sourcePath, StandardCharsets.UTF_8);
    final String sourceName = sourcePath.getFileName().toString();

    final ContextFactory factory =
        new ContextFactory() {
          @Override
          protected boolean hasFeature(Context cx, int featureIndex) {
            if (featureIndex == Context.FEATURE_ENABLE_JAVA_MAP_ACCESS) {
              return true;
            }
            return super.hasFeature(cx, featureIndex);
          }

          @Override
          protected Context makeContext() {
            Context cx = super.makeContext();
            cx.setOptimizationLevel(-1);
            cx.setInstructionObserverThreshold(1000);
            cx.setMaximumInterpreterStackDepth(10000);
            return cx;
          }
        };

    final Context cx = factory.enterContext();
    try {
      cx.setLanguageVersion(languageVersion);
      final Scriptable scope = cx.initStandardObjects();

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
      final ResultView result = ResultView.of(raw);

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
      json.append("\"result\":").append(result.json);
      json.append('}');
      System.out.println(json);
    } finally {
      Context.exit();
    }
  }

  static int parseLanguageVersion(String raw) {
    switch (raw) {
      case "DEFAULT":
      case "VERSION_DEFAULT":
        return Context.VERSION_DEFAULT;
      case "1.7":
      case "VERSION_1_7":
        return Context.VERSION_1_7;
      case "1.8":
      case "VERSION_1_8":
        return Context.VERSION_1_8;
      case "ES6":
      case "VERSION_ES6":
        return Context.VERSION_ES6;
      default:
        break;
    }
    try {
      int n = Integer.parseInt(raw);
      if (!Context.isValidLanguageVersion(n)) {
        throw new IllegalArgumentException("not a Rhino language version: " + raw);
      }
      return n;
    } catch (NumberFormatException e) {
      throw new IllegalArgumentException("not a Rhino language version: " + raw);
    }
  }

  private static void field(StringBuilder json, String name, String value, boolean quoted) {
    json.append('"').append(name).append("\":");
    if (value == null) {
      json.append("null");
    } else if (quoted) {
      json.append(jsonQuote(value));
    } else {
      json.append(value);
    }
  }

  static String jsonQuote(String s) {
    StringBuilder sb = new StringBuilder(s.length() + 2);
    sb.append('"');
    for (int i = 0; i < s.length(); i++) {
      char c = s.charAt(i);
      switch (c) {
        case '"':
          sb.append("\\\"");
          break;
        case '\\':
          sb.append("\\\\");
          break;
        case '\n':
          sb.append("\\n");
          break;
        case '\r':
          sb.append("\\r");
          break;
        case '\t':
          sb.append("\\t");
          break;
        default:
          if (c < 0x20) {
            sb.append(String.format("\\u%04x", (int) c));
          } else {
            sb.append(c);
          }
      }
    }
    sb.append('"');
    return sb.toString();
  }

  private static final class ResultView {
    final boolean defined;
    final String kind;
    final String json;

    ResultView(boolean defined, String kind, String json) {
      this.defined = defined;
      this.kind = kind;
      this.json = json;
    }

    static ResultView of(Object raw) {
      if (raw == Scriptable.NOT_FOUND) {
        return new ResultView(false, "missing", "null");
      }
      if (raw == null) {
        return new ResultView(true, "null", "null");
      }
      if (Undefined.isUndefined(raw)) {
        return new ResultView(true, "undefined", "null");
      }
      if (raw instanceof Boolean) {
        return new ResultView(true, "boolean", raw.toString());
      }
      if (raw instanceof Number) {
        return new ResultView(true, "number", raw.toString());
      }
      return new ResultView(true, "string", jsonQuote(Context.toString(raw)));
    }
  }
}
