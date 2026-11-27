import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.mozilla.javascript.CompilerEnvirons;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Parser;
import org.mozilla.javascript.Token;
import org.mozilla.javascript.ast.AstRoot;
import org.mozilla.javascript.ast.Symbol;

/**
 * The names a custom node's script declares at its own top level, read from Rhino's parse rather
 * than its text: {@code var} declarators (initialised or not) and function declarations, in the
 * script's own symbol table. A helper's locals live in the helper's table, and a declaration-like
 * string or comment is not a declaration at all, so neither is listed.
 *
 * <p>Parsed with the running context's settings, so the inventory matches how the script runs, and
 * cached by those settings and the source, so each script variant is parsed once per JVM.
 */
final class CustomDeclarations {

  private CustomDeclarations() {}

  private static final int MAX = 256;

  /** Access-ordered LRU; guarded by itself. */
  private static final LinkedHashMap<String, List<String>> CACHE =
      new LinkedHashMap<String, List<String>>(32, 0.75f, true) {
        @Override
        protected boolean removeEldestEntry(Map.Entry<String, List<String>> eldest) {
          return size() > MAX;
        }
      };

  static List<String> of(Context cx, String source) {
    CompilerEnvirons env = new CompilerEnvirons();
    env.initFromContext(cx);
    String key =
        env.getLanguageVersion()
            + (env.isStrictMode() ? "s" : "")
            + (env.isXmlAvailable() ? "x" : "")
            + "\u0000"
            + source;
    synchronized (CACHE) {
      List<String> hit = CACHE.get(key);
      if (hit != null) return hit;
    }
    AstRoot root = new Parser(env).parse(source, "custom node script", 1);
    List<String> names = new ArrayList<String>();
    Map<String, Symbol> table = root.getSymbolTable();
    if (table != null) {
      for (Symbol symbol : table.values()) {
        int type = symbol.getDeclType();
        if (type != Token.VAR && type != Token.FUNCTION) continue;
        names.add(symbol.getName());
      }
    }
    List<String> listed = java.util.Collections.unmodifiableList(names);
    synchronized (CACHE) {
      CACHE.put(key, listed);
    }
    return listed;
  }
}
