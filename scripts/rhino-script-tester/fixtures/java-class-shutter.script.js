// Probe: is "prohibited class" a quirk of reading identity attributes, or a
// general Rhino class-shutter policy? Safe to delete.
//
// docs/api/14-am-identity-attributes.md records `Set.iterator()` blocked with
// `java.util.ArrayList$Itr … prohibited` while reading an identity attribute,
// which reads as a quirk of that binding. The discriminating case is a
// collection the script CONSTRUCTS ITSELF: no binding, no identity, nothing
// AM handed us. If that is prohibited too, the rule belongs next to the Rhino
// language bans, not on the identity page.
//
// The second question is WHICH classes. `scripted-decision-next.json` lists
// `java.util.HashMap$KeyIterator` and `java.util.Collections$UnmodifiableCollection$1`
// — both iterators — but not `java.util.ArrayList$Itr`. If the allow-list is
// what is enforced then `HashSet.iterator()` (a `HashMap$KeyIterator`) WORKS
// while `ArrayList.iterator()` does not, and "iterators are banned" is the
// wrong mental model. Every constructor below is already known to work here
// (see fixtures/java-collections.script.js), so a failure is about the
// iterator, not the collection.
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

function makeArrayList() {
  var l = new java.util.ArrayList();
  l.add("a");
  l.add("b");
  return l;
}

function makeHashSet() {
  var s = new java.util.HashSet();
  s.add("a");
  return s;
}

function makeLinkedHashSet() {
  var s = new java.util.LinkedHashSet();
  s.add("a");
  return s;
}

function makeTreeSet() {
  var s = new java.util.TreeSet();
  s.add("a");
  return s;
}

try {
  var results = [];

  // --- CONTROL. The documented workaround, on a self-made list. If this
  // fails, nothing below means anything.
  results.push(
    probe("CONTROL ArrayList.toArray index loop", function () {
      var items = makeArrayList().toArray();
      var out = [];
      for (var i = 0; i < items.length; i++) {
        out.push(String(items[i]));
      }
      return out.join(",");
    })
  );
  results.push(
    probe("CONTROL ArrayList.get(0)", function () {
      return makeArrayList().get(0);
    })
  );
  results.push(
    probe("CONTROL ArrayList.size()", function () {
      return makeArrayList().size();
    })
  );

  // --- THE DISCRIMINATOR. A list this script built, iterated.
  results.push(
    probe("ArrayList.iterator()", function () {
      return makeArrayList().iterator().next();
    })
  );

  // --- Which classes? HashSet's iterator IS on the declared allow-list
  // (`java.util.HashMap$KeyIterator`); ArrayList's is not.
  results.push(
    probe("HashSet.iterator()", function () {
      return makeHashSet().iterator().next();
    })
  );
  results.push(
    probe("LinkedHashSet.iterator()", function () {
      return makeLinkedHashSet().iterator().next();
    })
  );
  results.push(
    probe("TreeSet.iterator()", function () {
      return makeTreeSet().iterator().next();
    })
  );
  results.push(
    probe("Collections.singletonMap keySet().iterator()", function () {
      return java.util.Collections.singletonMap("a", 1).keySet().iterator().next();
    })
  );
  results.push(
    probe("Collections.unmodifiableList iterator()", function () {
      return java.util.Collections.unmodifiableList(makeArrayList()).iterator().next();
    })
  );

  // --- Is JSON.stringify a safe way to look at a Java object? This is the
  // first thing anyone reaches for in a debug session.
  results.push(
    probe("JSON.stringify(ArrayList)", function () {
      return JSON.stringify(makeArrayList());
    })
  );
  results.push(
    probe("JSON.stringify(HashSet)", function () {
      return JSON.stringify(makeHashSet());
    })
  );
  results.push(
    probe("JSON.stringify(LinkedHashSet)", function () {
      return JSON.stringify(makeLinkedHashSet());
    })
  );
  results.push(
    probe("JSON.stringify(singletonMap)", function () {
      return JSON.stringify(java.util.Collections.singletonMap("a", 1));
    })
  );

  // --- Safe alternatives to reach for instead.
  results.push(
    probe("String(ArrayList)", function () {
      return String(makeArrayList());
    })
  );
  results.push(
    probe("String(LinkedHashSet)", function () {
      return String(makeLinkedHashSet());
    })
  );
  results.push(
    probe("Object.keys(ArrayList)", function () {
      return Object.keys(makeArrayList()).join(",");
    })
  );
  results.push(
    probe("for-in over ArrayList", function () {
      var keys = [];
      var l = makeArrayList();
      for (var k in l) {
        keys.push(k);
      }
      return keys.length + " keys";
    })
  );
  results.push(
    probe("ArrayList.getClass().getName()", function () {
      return makeArrayList().getClass().getName();
    })
  );

  emit({ ok: true, feature: "java-class-shutter", value: results });
} catch (e) {
  emit({ ok: false, feature: "java-class-shutter", error: String(e) });
}
