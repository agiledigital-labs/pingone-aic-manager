import java.util.ArrayList;
import java.util.List;
import org.mozilla.javascript.ClassShutter;

/**
 * Replica of the AM script sandbox's per-context Java class allow-list.
 *
 * <p>AM runs Rhino behind a class shutter that decides, per class and per
 * script context, whether a Java class is visible at all. The local runner had
 * none, so every Java name resolved and every iterator worked — which made the
 * harness strictly MORE permissive than the tenant. A script using
 * {@code new java.util.HashMap()} or {@code list.iterator()} passed locally and
 * is refused by AIC.
 *
 * <p>The policy is not invented here. It is the {@code allowLists} array of the
 * context's binding descriptor (for a scripted decision node,
 * {@code docs/api/bindings/scripted-decision-next.json} — 51 entries), passed in
 * per job so a second context can supply its own list.
 *
 * <p>Rhino gives both of AM's observed failure shapes from this one hook, at no
 * extra effort — see {@code docs/api/12-script-bindings-matrix.md}:
 *
 * <ul>
 *   <li>a name the script writes itself never resolves past the package object,
 *       so {@code new java.util.HashMap()} fails as
 *       {@code TypeError: [JavaPackage java.util.HashMap] is not a function};
 *   <li>an instance handed to the script by a call or binding throws
 *       {@code InternalError: Access to Java class "…" is prohibited.}
 * </ul>
 *
 * <p>Patterns are exact names, or a trailing {@code *} meaning prefix. That one
 * rule covers every form the descriptor uses: {@code java.util.Collections$*},
 * {@code com.sun.proxy.$*} and
 * {@code org.forgerock.openam.core.rest.authn.callbackhandlers.*}.
 */
final class AmClassShutter implements ClassShutter {

  private final String[] exact;
  private final String[] prefixes;

  private AmClassShutter(String[] exact, String[] prefixes) {
    this.exact = exact;
    this.prefixes = prefixes;
  }

  /** Build from the descriptor's `allowLists` entries. */
  static AmClassShutter of(List<String> patterns) {
    List<String> exactNames = new ArrayList<String>();
    List<String> prefixNames = new ArrayList<String>();
    for (String pattern : patterns) {
      if (pattern == null || pattern.isEmpty()) {
        continue;
      }
      if (pattern.endsWith("*")) {
        prefixNames.add(pattern.substring(0, pattern.length() - 1));
      } else {
        exactNames.add(pattern);
      }
    }
    return new AmClassShutter(
        exactNames.toArray(new String[0]), prefixNames.toArray(new String[0]));
  }

  /**
   * Rhino's own runtime classes, which the shutter must never hide.
   *
   * <p>The shutter is consulted for engine internals as well as for classes a
   * script names. Hiding them does not enforce policy, it breaks the language:
   * a template literal produces an {@code org.mozilla.javascript.ConsString},
   * so a shutter built from the allow-list alone fails
   * {@code `hi ${who}`} with {@code Access to Java class
   * "org.mozilla.javascript.ConsString" is prohibited} — and template literals
   * are used by 181 of the 384 scripts in the production corpus and run fine on
   * AIC. AM is itself running Rhino, so its shutter cannot be hiding these
   * either.
   *
   * <p>This is a requirement of the engine, not an inference about AM's policy.
   */
  private static final String ENGINE_PACKAGE = "org.mozilla.javascript.";

  @Override
  public boolean visibleToScripts(String fullClassName) {
    if (fullClassName.startsWith(ENGINE_PACKAGE)) {
      return true;
    }
    for (String name : exact) {
      if (name.equals(fullClassName)) {
        return true;
      }
    }
    for (String prefix : prefixes) {
      if (fullClassName.startsWith(prefix)) {
        return true;
      }
    }
    return false;
  }
}
