import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Minimal JSON parse/stringify for the runner protocol. No extra jars: the host
 * has no JDK and the AM image classpath for this harness is Rhino only.
 */
final class Json {

  private Json() {}

  static Object parse(String source) {
    return new Parser(source).parse();
  }

  static String stringify(Object value) {
    StringBuilder sb = new StringBuilder();
    write(sb, value);
    return sb.toString();
  }

  static String quote(String s) {
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
        case '\b':
          sb.append("\\b");
          break;
        case '\f':
          sb.append("\\f");
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

  @SuppressWarnings("unchecked")
  static Map<String, Object> asObject(Object value, String what) {
    if (!(value instanceof Map)) {
      throw new IllegalArgumentException(what + " must be a JSON object");
    }
    return (Map<String, Object>) value;
  }

  static String asString(Object value, String what) {
    if (!(value instanceof String)) {
      throw new IllegalArgumentException(what + " must be a string");
    }
    return (String) value;
  }

  static String optionalString(Map<String, Object> obj, String key) {
    if (!obj.containsKey(key) || obj.get(key) == null) {
      return null;
    }
    return asString(obj.get(key), key);
  }

  static long optionalLong(Map<String, Object> obj, String key, long defaultValue) {
    if (!obj.containsKey(key) || obj.get(key) == null) {
      return defaultValue;
    }
    Object raw = obj.get(key);
    if (raw instanceof Number) {
      return ((Number) raw).longValue();
    }
    throw new IllegalArgumentException(key + " must be a number");
  }

  /** A JSON array of strings, or null when the key is absent. */
  static List<String> optionalStringList(Map<String, Object> obj, String key) {
    Object value = obj.get(key);
    if (value == null) {
      return null;
    }
    if (!(value instanceof List)) {
      throw new IllegalArgumentException(key + " must be an array of strings");
    }
    List<String> out = new ArrayList<String>();
    for (Object item : (List<?>) value) {
      if (!(item instanceof String)) {
        throw new IllegalArgumentException(key + " must contain only strings");
      }
      out.add((String) item);
    }
    return out;
  }

  static boolean optionalBoolean(Map<String, Object> obj, String key, boolean defaultValue) {
    if (!obj.containsKey(key) || obj.get(key) == null) {
      return defaultValue;
    }
    Object raw = obj.get(key);
    if (raw instanceof Boolean) {
      return ((Boolean) raw).booleanValue();
    }
    throw new IllegalArgumentException(key + " must be a boolean");
  }

  private static void write(StringBuilder sb, Object value) {
    if (value == null) {
      sb.append("null");
      return;
    }
    if (value instanceof String) {
      sb.append(quote((String) value));
      return;
    }
    if (value instanceof Boolean) {
      sb.append(value.toString());
      return;
    }
    if (value instanceof Number) {
      Number n = (Number) value;
      double d = n.doubleValue();
      if (Double.isNaN(d) || Double.isInfinite(d)) {
        sb.append("null");
        return;
      }
      if (n instanceof Double || n instanceof Float) {
        sb.append(n.toString());
        return;
      }
      sb.append(Long.toString(n.longValue()));
      return;
    }
    if (value instanceof Map) {
      sb.append('{');
      boolean first = true;
      for (Map.Entry<?, ?> e : ((Map<?, ?>) value).entrySet()) {
        if (!first) {
          sb.append(',');
        }
        first = false;
        sb.append(quote(String.valueOf(e.getKey())));
        sb.append(':');
        write(sb, e.getValue());
      }
      sb.append('}');
      return;
    }
    if (value instanceof List) {
      sb.append('[');
      boolean first = true;
      for (Object item : (List<?>) value) {
        if (!first) {
          sb.append(',');
        }
        first = false;
        write(sb, item);
      }
      sb.append(']');
      return;
    }
    sb.append(quote(String.valueOf(value)));
  }

  private static final class Parser {
    private final String src;
    private final int length;
    private int pos;

    Parser(String src) {
      this.src = src;
      this.length = src.length();
    }

    Object parse() {
      Object value = readValue();
      skipWs();
      if (pos != length) {
        throw error("trailing junk after JSON value");
      }
      return value;
    }

    private Object readValue() {
      skipWs();
      if (pos >= length) {
        throw error("unexpected end of JSON");
      }
      char c = src.charAt(pos);
      switch (c) {
        case '{':
          return readObject();
        case '[':
          return readArray();
        case '"':
          return readString();
        case 't':
          return readLiteral("true", Boolean.TRUE);
        case 'f':
          return readLiteral("false", Boolean.FALSE);
        case 'n':
          return readLiteral("null", null);
        case '-':
          return readNumber();
        default:
          if (c >= '0' && c <= '9') {
            return readNumber();
          }
          throw error("unexpected character '" + c + "'");
      }
    }

    private Map<String, Object> readObject() {
      consume('{');
      Map<String, Object> obj = new LinkedHashMap<String, Object>();
      skipWs();
      if (peek('}')) {
        consume('}');
        return obj;
      }
      while (true) {
        skipWs();
        if (pos >= length || src.charAt(pos) != '"') {
          throw error("expected string key");
        }
        String key = readString();
        skipWs();
        consume(':');
        obj.put(key, readValue());
        skipWs();
        if (peek('}')) {
          consume('}');
          return obj;
        }
        consume(',');
      }
    }

    private List<Object> readArray() {
      consume('[');
      List<Object> arr = new ArrayList<Object>();
      skipWs();
      if (peek(']')) {
        consume(']');
        return arr;
      }
      while (true) {
        arr.add(readValue());
        skipWs();
        if (peek(']')) {
          consume(']');
          return arr;
        }
        consume(',');
      }
    }

    private String readString() {
      consume('"');
      StringBuilder sb = new StringBuilder();
      while (pos < length) {
        char c = src.charAt(pos++);
        if (c == '"') {
          return sb.toString();
        }
        if (c == '\\') {
          if (pos >= length) {
            throw error("unterminated escape");
          }
          char e = src.charAt(pos++);
          switch (e) {
            case '"':
            case '\\':
            case '/':
              sb.append(e);
              break;
            case 'b':
              sb.append('\b');
              break;
            case 'f':
              sb.append('\f');
              break;
            case 'n':
              sb.append('\n');
              break;
            case 'r':
              sb.append('\r');
              break;
            case 't':
              sb.append('\t');
              break;
            case 'u':
              if (pos + 4 > length) {
                throw error("truncated \\u escape");
              }
              int cp = 0;
              for (int i = 0; i < 4; i++) {
                cp = (cp << 4) | fromHex(src.charAt(pos++));
              }
              sb.append((char) cp);
              break;
            default:
              throw error("bad escape \\" + e);
          }
        } else if (c < 0x20) {
          throw error("unescaped control character in string");
        } else {
          sb.append(c);
        }
      }
      throw error("unterminated string");
    }

    private Object readLiteral(String lit, Object value) {
      if (pos + lit.length() > length || !src.startsWith(lit, pos)) {
        throw error("expected " + lit);
      }
      pos += lit.length();
      return value;
    }

    private Number readNumber() {
      int start = pos;
      if (peek('-')) {
        pos++;
      }
      if (pos >= length) {
        throw error("expected number");
      }
      if (src.charAt(pos) == '0') {
        pos++;
      } else if (src.charAt(pos) >= '1' && src.charAt(pos) <= '9') {
        while (pos < length && isDigit(src.charAt(pos))) {
          pos++;
        }
      } else {
        throw error("expected digit");
      }
      boolean frac = false;
      if (peek('.')) {
        frac = true;
        pos++;
        if (pos >= length || !isDigit(src.charAt(pos))) {
          throw error("expected digit after decimal point");
        }
        while (pos < length && isDigit(src.charAt(pos))) {
          pos++;
        }
      }
      if (pos < length && (src.charAt(pos) == 'e' || src.charAt(pos) == 'E')) {
        frac = true;
        pos++;
        if (pos < length && (src.charAt(pos) == '+' || src.charAt(pos) == '-')) {
          pos++;
        }
        if (pos >= length || !isDigit(src.charAt(pos))) {
          throw error("expected digit in exponent");
        }
        while (pos < length && isDigit(src.charAt(pos))) {
          pos++;
        }
      }
      String raw = src.substring(start, pos);
      if (!frac) {
        try {
          long n = Long.parseLong(raw);
          if (n >= Integer.MIN_VALUE && n <= Integer.MAX_VALUE) {
            return Integer.valueOf((int) n);
          }
          return Long.valueOf(n);
        } catch (NumberFormatException e) {
          // fall through to double
        }
      }
      return Double.valueOf(raw);
    }

    private void skipWs() {
      while (pos < length) {
        char c = src.charAt(pos);
        if (c == ' ' || c == '\n' || c == '\r' || c == '\t') {
          pos++;
        } else {
          return;
        }
      }
    }

    private boolean peek(char c) {
      return pos < length && src.charAt(pos) == c;
    }

    private void consume(char c) {
      skipWs();
      if (pos >= length || src.charAt(pos) != c) {
        throw error("expected '" + c + "'");
      }
      pos++;
    }

    private IllegalArgumentException error(String msg) {
      return new IllegalArgumentException("JSON at " + pos + ": " + msg);
    }

    private static boolean isDigit(char c) {
      return c >= '0' && c <= '9';
    }

    private int fromHex(char c) {
      if (c >= '0' && c <= '9') {
        return c - '0';
      }
      if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
      }
      if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
      }
      throw error("bad hex digit '" + c + "'");
    }
  }
}
