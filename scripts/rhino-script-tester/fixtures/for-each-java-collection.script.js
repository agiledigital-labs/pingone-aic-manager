// Probe: does `for each (var x in coll)` reach the iterator, and is that
// enough to trip the class shutter? Safe to delete.
//
// Kept apart from java-class-shutter.script.js deliberately: `for each` is E4X
// syntax, so if Rhino rejects it the WHOLE script fails to parse and the
// harness records `no-callback`. Bundling it would have taken every other
// probe down with it.
//
// Reading the result:
//   no-callback              -> `for each` is a parse error here; ban the syntax
//   js-array ok, java not ok -> it parses and desugars to .iterator(), which
//                               the class shutter blocks on a Java collection
//   both ok                  -> `for each` is fine and the report's advice to
//                               avoid it is over-broad
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

  // CONTROL: the same syntax over a plain JS array. If this fails the syntax
  // is the problem, not the collection.
  results.push(
    probe("for each over JS array", function () {
      var out = [];
      var arr = ["a", "b"];
      for each (var v in arr) {
        out.push(String(v));
      }
      return out.join(",");
    })
  );

  results.push(
    probe("for each over java.util.ArrayList", function () {
      var out = [];
      var l = new java.util.ArrayList();
      l.add("a");
      l.add("b");
      for each (var v in l) {
        out.push(String(v));
      }
      return out.join(",");
    })
  );

  results.push(
    probe("for each over java.util.HashSet", function () {
      var out = [];
      var s = new java.util.HashSet();
      s.add("a");
      for each (var v in s) {
        out.push(String(v));
      }
      return out.join(",");
    })
  );

  emit({ ok: true, feature: "for-each-java-collection", value: results });
} catch (e) {
  emit({ ok: false, feature: "for-each-java-collection", error: String(e) });
}
