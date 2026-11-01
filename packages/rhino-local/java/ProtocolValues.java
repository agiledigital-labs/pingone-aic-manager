import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Out-of-band numeric paths preserve internal non-finite numbers in JSON frames. */
final class ProtocolValues {
  static Object encode(Map<String, Object> record) {
    List<Object> numbers = new ArrayList<Object>();
    Map<String, Object> encoded = Json.asObject(encodeValue(record, new ArrayList<String>(), numbers), "frame");
    if (!numbers.isEmpty()) encoded.put("_rhinoNumbers", numbers);
    return encoded;
  }
  private static Object encodeValue(Object value, List<String> path, List<Object> numbers) {
    if (value instanceof Double || value instanceof Float) {
      double number = ((Number) value).doubleValue();
      if (!Double.isFinite(number) || Double.doubleToRawLongBits(number) == Double.doubleToRawLongBits(-0.0)) {
        Map<String, Object> entry = new LinkedHashMap<String, Object>();
        entry.put("path", path);
        entry.put("kind", Double.isNaN(number) ? "NaN" : number == 0 ? "-0" : number > 0 ? "Infinity" : "-Infinity");
        numbers.add(entry);
        return null;
      }
    }
    if (value instanceof Map) {
      Map<String, Object> result = new LinkedHashMap<String, Object>();
      for (Map.Entry<String, Object> entry : Json.asObject(value, "map").entrySet()) {
        List<String> childPath = new ArrayList<String>(path); childPath.add(entry.getKey());
        result.put(entry.getKey(), encodeValue(entry.getValue(), childPath, numbers));
      }
      return result;
    }
    if (value instanceof List) {
      List<Object> result = new ArrayList<Object>();
      for (Object child : (List<?>) value) {
        List<String> childPath = new ArrayList<String>(path); childPath.add(Integer.toString(result.size()));
        result.add(encodeValue(child, childPath, numbers));
      }
      return result;
    }
    return value;
  }
  @SuppressWarnings("unchecked")
  static Object decode(Object value) {
    if (!(value instanceof Map)) return value;
    Map<String, Object> record = Json.asObject(value, "frame");
    Object raw = record.remove("_rhinoNumbers");
    if (raw == null) return record;
    for (Object entry : (List<?>) raw) {
      Map<String, Object> number = Json.asObject(entry, "numeric path");
      List<String> path = Json.optionalStringList(number, "path");
      String kind = Json.asString(number.get("kind"), "numeric kind");
      double restored;
      if ("NaN".equals(kind)) restored = Double.NaN;
      else if ("Infinity".equals(kind)) restored = Double.POSITIVE_INFINITY;
      else if ("-Infinity".equals(kind)) restored = Double.NEGATIVE_INFINITY;
      else if ("-0".equals(kind)) restored = -0.0;
      else throw new IllegalArgumentException("invalid protocol numeric value");
      Object target = record;
      for (int i = 0; i < path.size() - 1; i++) target = target instanceof Map
          ? Json.asObject(target, "numeric parent").get(path.get(i)) : ((List<?>) target).get(Integer.parseInt(path.get(i)));
      String key = path.get(path.size() - 1);
      if (target instanceof Map) Json.asObject(target, "numeric target").put(key, Double.valueOf(restored));
      else ((List<Object>) target).set(Integer.parseInt(key), Double.valueOf(restored));
    }
    return record;
  }
}
