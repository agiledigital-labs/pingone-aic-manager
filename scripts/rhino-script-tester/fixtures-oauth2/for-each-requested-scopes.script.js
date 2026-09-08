// D4: is `for each (var s in requestedScopes)` really unusable here?
//
// The report says it is, on the reasoning that `for each` desugars to
// `.iterator()` and iterators are blocked. The decision-node lane already
// refutes half of that — `for each` over a self-made `ArrayList` iterates
// fine there while `ArrayList.iterator()` is prohibited — so `for each` is
// not simply sugar for the blocked call. `requestedScopes` is the binding the
// report actually used, so it is the case that decides the doc row.
//
// Kept out of `java-class-shutter.script.js` because `for each` is E4X: if it
// fails to PARSE the whole script dies, and it would take the battery with it.
// A parse failure here shows up as a non-200 with no AICEDIT line logged at
// all, which is distinguishable from a caught runtime throw.
function asArray(collection) {
  var out = [];
  var items = collection.toArray();
  for (var i = 0; i < items.length; i++) {
    out.push(String(items[i]));
  }
  return out;
}

function probe(name, fn) {
  var line;
  try {
    line = "ok value=" + String(fn());
  } catch (e) {
    line = "FAILED " + String(e);
  }
  logger.error("AICEDIT-D4 [" + name + "] " + line);
}

function validateAccessTokenScope() {
  probe("for each over JS array (control)", function () {
    var out = [];
    var arr = ["a", "b"];
    for each (var v in arr) {
      out.push(String(v));
    }
    return out.join(",");
  });
  probe("for each over requestedScopes", function () {
    var out = [];
    for each (var s in requestedScopes) {
      out.push(String(s));
    }
    return out.join(",");
  });
  return asArray(requestedScopes);
}
