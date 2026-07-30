// LIBRARY-context probe for java.util access. Uploaded to the sandbox as
// `rhino-lib-java-collections-probe` (script id …7408) and consumed by
// lib-java-collections-consumer.script.js. Safe to delete.
//
// The LIBRARY context's binding metadata declares an allow-list of just THREE
// classes (java.lang.Object + the two promise types) against
// SCRIPTED_DECISION_NODE's 51 — see docs/api/bindings/library-next.json. If the
// allow-list is enforced per compiling context rather than per calling context,
// a library cannot use the java.util collections its caller can, which decides
// whether shared helpers can lean on them at all.
function probe(name, fn) {
  try {
    return { name: name, ok: true, value: String(fn()) };
  } catch (e) {
    return { name: name, ok: false, error: String(e) };
  }
}

exports.typeofJavaImporter = typeof JavaImporter;
exports.typeofJava = typeof java;

exports.results = [
  probe("new java.util.HashSet", function () {
    var s = new java.util.HashSet();
    s.add("a");
    return s.contains("a") + ":" + s.size();
  }),
  probe("new java.util.ArrayList", function () {
    var l = new java.util.ArrayList();
    l.add("a");
    return l.get(0) + ":" + l.size();
  }),
  probe("new java.util.HashMap", function () {
    var m = new java.util.HashMap();
    m.put("a", 1);
    return m.get("a") + ":" + m.size();
  }),
  probe("java.util.Collections.singletonMap", function () {
    var m = java.util.Collections.singletonMap("a", 1);
    return m.get("a") + ":" + m.size();
  }),
];
