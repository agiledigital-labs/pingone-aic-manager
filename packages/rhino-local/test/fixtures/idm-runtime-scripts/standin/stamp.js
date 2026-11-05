// Invented stand-in for rhino-local tests; not ForgeRock code. Run as an
// object hook through a `file` reference.
object.stamp = "stamped:" + request.method + ":" + String(resourceName);
