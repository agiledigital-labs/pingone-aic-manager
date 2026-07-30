// Probe: which java.util collections a NEXT-GEN script can actually construct.
// Safe to delete.
//
// Next-gen has a fixed (non-configurable) Java allow-list, exposed as the
// `allowLists` array in each context's binding metadata — see
// docs/api/bindings/scripted-decision-next.json. For SCRIPTED_DECISION_NODE that
// list has 51 entries and includes java.util.HashSet, ArrayList, LinkedHashSet,
// TreeSet and LinkedList — but NOT java.util.HashMap itself (only
// java.util.HashMap$KeyIterator and java.util.AbstractMap$*). This fixture checks
// whether the declared list matches enforcement, since Map is the interesting
// case: it is the substitute we would point people at for the missing JS `Map`.
//
// Both access styles are probed: the fully-qualified `java.util.X` global path and
// the JavaImporter path, because they can be gated differently.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function probe(name, fn) {
  try {
    return { name: name, ok: true, value: String(fn()) };
  } catch (e) {
    return { name: name, ok: false, error: String(e) };
  }
}

try {
  var results = [];
  results.push(
    probe("typeof JavaImporter", function () {
      return typeof JavaImporter;
    })
  );
  results.push(
    probe("typeof java", function () {
      return typeof java;
    })
  );
  // On the allow-list.
  results.push(
    probe("new java.util.HashSet", function () {
      var s = new java.util.HashSet();
      s.add("a");
      s.add("a");
      return s.contains("a") + ":" + s.size();
    })
  );
  results.push(
    probe("new java.util.ArrayList", function () {
      var l = new java.util.ArrayList();
      l.add("a");
      return l.get(0) + ":" + l.size();
    })
  );
  results.push(
    probe("new java.util.LinkedHashSet", function () {
      var s = new java.util.LinkedHashSet();
      s.add("a");
      return String(s.size());
    })
  );
  results.push(
    probe("new java.util.TreeSet", function () {
      var s = new java.util.TreeSet();
      s.add("b");
      s.add("a");
      return s.first() + ":" + s.size();
    })
  );
  // NOT on the allow-list by name — the key question.
  results.push(
    probe("new java.util.HashMap", function () {
      var m = new java.util.HashMap();
      m.put("a", 1);
      return m.get("a") + ":" + m.size();
    })
  );
  results.push(
    probe("new java.util.LinkedHashMap", function () {
      var m = new java.util.LinkedHashMap();
      m.put("a", 1);
      return m.get("a") + ":" + m.size();
    })
  );
  results.push(
    probe("new java.util.TreeMap", function () {
      var m = new java.util.TreeMap();
      m.put("a", 1);
      return m.get("a") + ":" + m.size();
    })
  );
  // On the allow-list, and returns a Map without naming HashMap.
  results.push(
    probe("java.util.Collections.emptyMap", function () {
      return String(java.util.Collections.emptyMap().size());
    })
  );
  results.push(
    probe("java.util.Collections.singletonMap", function () {
      var m = java.util.Collections.singletonMap("a", 1);
      return m.get("a") + ":" + m.size();
    })
  );
  // The JavaImporter path to the same classes.
  results.push(
    probe("JavaImporter HashMap", function () {
      var ju = JavaImporter(java.util);
      var m = new ju.HashMap();
      m.put("a", 1);
      return m.get("a") + ":" + m.size();
    })
  );
  results.push(
    probe("JavaImporter HashSet", function () {
      var ju = JavaImporter(java.util);
      var s = new ju.HashSet();
      s.add("a");
      return String(s.size());
    })
  );
  emit({ ok: true, feature: "java-collections", value: results });
} catch (e) {
  emit({ ok: false, feature: "java-collections", error: String(e) });
}
