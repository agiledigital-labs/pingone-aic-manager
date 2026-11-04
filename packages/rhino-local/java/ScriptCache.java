import java.lang.reflect.Field;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Supplier;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.Script;

/**
 * Content-addressed sources and the compiled scripts made from them.
 *
 * <p><b>What is shared, and why that is safe.</b> A job still gets a fresh
 * {@code initStandardObjects()} scope and still <em>executes</em> every
 * script into it, so every object a script can reach — globals, builtin
 * prototypes, binding objects, a library's module state — is created anew per
 * job. Only the compiled form is shared: Rhino's interpreter keeps a script's
 * code in an {@code InterpreterData} that execution reads and does not write,
 * and a regular-expression literal stores its compiled program there and
 * wraps a new {@code RegExp} on every evaluation.
 *
 * <p>The one exception found in Rhino 1.7.14.1 is the tagged template literal:
 * {@code ScriptRuntime.getTemplateLiteralCallSite} replaces the raw strings in
 * the script's {@code itsTemplateLiterals} with the call-site array it built,
 * which then carries the first job's {@code Array.prototype} into every later
 * one. A script with any template-literal table is therefore compiled per job
 * and never cached ({@link #cacheable}). Anything that is not an interpreted
 * script (another optimisation level) is likewise not cached, so an unknown
 * shape fails towards the old behaviour rather than towards sharing.
 */
final class ScriptCache {

  private ScriptCache() {}

  /** Sources the client has shipped, by SHA-256 hex. Lives as long as the JVM. */
  private static final Map<String, Source> SOURCES = new ConcurrentHashMap<String, Source>();

  static final class Source {
    final String text;
    /** Newlines in {@link #text}: a script that continues this one starts on line {@code lines + 1}. */
    final int lines;

    Source(String text) {
      this.text = text;
      int n = 0;
      for (int i = 0; i < text.length(); i += 1) if (text.charAt(i) == '\n') n += 1;
      this.lines = n;
    }
  }

  private static final int MAX_COMPILED = 1024;

  /** Access-ordered LRU; guarded by itself. */
  private static final LinkedHashMap<String, Script> COMPILED =
      new LinkedHashMap<String, Script>(64, 0.75f, true) {
        @Override
        protected boolean removeEldestEntry(Map.Entry<String, Script> eldest) {
          return size() > MAX_COMPILED;
        }
      };

  /**
   * Register a source the client shipped. Called on the reader thread, in line
   * order, so a later job naming the hash always finds it.
   */
  static void register(String hash, String text) {
    if (SOURCES.containsKey(hash)) return;
    if (!hash.equals(sha256(text))) {
      throw new IllegalArgumentException("source does not match its hash " + hash);
    }
    SOURCES.putIfAbsent(hash, new Source(text));
  }

  static Source source(String hash) {
    Source source = SOURCES.get(hash);
    if (source == null) throw new IllegalArgumentException("unknown source " + hash);
    return source;
  }

  /**
   * The compiled form of {@code text}, compiled with {@code cx}'s settings.
   * {@code identity} must name the text uniquely (a hash plus any wrapper).
   */
  static Script compile(Context cx, String identity, Supplier<String> text, String name, int line) {
    // Everything that changes the icode Rhino emits, plus the name and line it bakes in.
    String key = identity + '\0' + name + '\0' + line + '\0' + cx.getLanguageVersion() + '\0'
        + cx.getOptimizationLevel() + '\0' + (cx.getInstructionObserverThreshold() > 0) + '\0'
        + cx.isGeneratingDebug() + '\0' + cx.isGeneratingSource();
    synchronized (COMPILED) {
      Script hit = COMPILED.get(key);
      if (hit != null) {
        return hit;
      }
    }
    Script script = cx.compileString(text.get(), name, line, null);
    if (cacheable(script)) {
      synchronized (COMPILED) {
        Script raced = COMPILED.get(key);
        if (raced != null) return raced;
        COMPILED.put(key, script);
      }
    }
    return script;
  }

  private static final Field IDATA;
  private static final Field TEMPLATES;
  private static final Field NESTED;

  static {
    Field idata = null;
    Field templates = null;
    Field nested = null;
    try {
      idata = Class.forName("org.mozilla.javascript.InterpretedFunction").getDeclaredField("idata");
      Class<?> data = Class.forName("org.mozilla.javascript.InterpreterData");
      templates = data.getDeclaredField("itsTemplateLiterals");
      nested = data.getDeclaredField("itsNestedFunctions");
      idata.setAccessible(true);
      templates.setAccessible(true);
      nested.setAccessible(true);
    } catch (ReflectiveOperationException | RuntimeException e) {
      idata = null;
    }
    IDATA = idata;
    TEMPLATES = templates;
    NESTED = nested;
  }

  /** True only for an interpreted script none of whose functions has a template-literal table. */
  static boolean cacheable(Script script) {
    if (IDATA == null || !IDATA.getDeclaringClass().isInstance(script)) return false;
    try {
      return pure(IDATA.get(script));
    } catch (IllegalAccessException e) {
      return false;
    }
  }

  private static boolean pure(Object data) throws IllegalAccessException {
    if (data == null) return true;
    Object[] templates = (Object[]) TEMPLATES.get(data);
    if (templates != null && templates.length > 0) return false;
    Object[] nested = (Object[]) NESTED.get(data);
    if (nested != null) for (Object child : nested) if (!pure(child)) return false;
    return true;
  }

  /**
   * SHA-256 over the UTF-16LE code units, exactly as the client hashes
   * {@code Buffer.from(text, "utf16le")}: no encoder sits in between, so a lone
   * surrogate hashes the same on both sides instead of being replaced.
   */
  static String sha256(String text) {
    try {
      byte[] units = new byte[text.length() * 2];
      for (int i = 0; i < text.length(); i += 1) {
        char c = text.charAt(i);
        units[2 * i] = (byte) c;
        units[2 * i + 1] = (byte) (c >> 8);
      }
      byte[] digest = MessageDigest.getInstance("SHA-256").digest(units);
      StringBuilder hex = new StringBuilder(digest.length * 2);
      for (byte b : digest) hex.append(Character.forDigit((b >> 4) & 0xf, 16)).append(Character.forDigit(b & 0xf, 16));
      return hex.toString();
    } catch (NoSuchAlgorithmException e) {
      throw new IllegalStateException(e);
    }
  }
}
