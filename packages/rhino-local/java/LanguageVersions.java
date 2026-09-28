import org.mozilla.javascript.Context;

/** Parse a Rhino {@link Context} language-version token the way {@code Probe} does. */
final class LanguageVersions {

  private LanguageVersions() {}

  static int parse(String raw) {
    if (raw == null) {
      return Context.VERSION_DEFAULT;
    }
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

  static int parse(Object raw) {
    if (raw == null) {
      return Context.VERSION_DEFAULT;
    }
    if (raw instanceof Number) {
      int n = ((Number) raw).intValue();
      if (!Context.isValidLanguageVersion(n)) {
        throw new IllegalArgumentException("not a Rhino language version: " + raw);
      }
      return n;
    }
    if (raw instanceof String) {
      return parse((String) raw);
    }
    throw new IllegalArgumentException("languageVersion must be a number or string");
  }
}
