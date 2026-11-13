import java.util.LinkedHashMap;
import java.util.Map;

/** Not a tenant exception. With IDM feature 13 off, Rhino cannot catch Java Errors. */
public final class EmulationFailure extends Error {
  final Map<String, Object> info;
  EmulationFailure(Map<String, Object> info) {
    super(Json.optionalString(info, "message"));
    this.info = info;
  }
  EmulationFailure(String kind, String message) {
    super(message);
    info = new LinkedHashMap<String, Object>();
    info.put("kind", kind);
    info.put("message", message);
  }
}
