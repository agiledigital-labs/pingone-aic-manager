// D4: the same class-shutter battery as `fixtures/java-class-shutter.script.js`,
// re-run in the next-gen OAUTH2_VALIDATE_SCOPE context.
//
// Two questions the decision-node lane cannot answer:
//
//   1. Is the ENFORCED Java allow-list the same in every next-gen context?
//      `12-script-bindings-matrix.md` claims it is, and that it corresponds to
//      the 51-entry `scripted-decision-next.json` list. This context declares
//      only THREE entries, and the D4 report saw `java.util.LinkedHashSet`
//      prohibited here — a class that is on the 51 and demonstrably usable in
//      a decision node. The self-constructed probes below are the control that
//      settles it.
//   2. What happens on the BINDINGS — `requestedScopes`, `clientProperties`,
//      `requestProperties` — which only exist here.
//
// Every result is logged one line per probe, tagged AICEDIT-D4, because a
// validate-scope script's only return channel is the scope list. Read them
// back with `aic logs tx <txid>`.
//
// NEVER log the VALUE of a stringify or toString on `clientProperties` or
// `requestProperties`: the request carried `client_secret`, and the client
// profile carries `userpassword`. Length and success/failure answer the
// question; the contents would put a secret in the tenant's log.
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

function keysOf(obj) {
  var keys = [];
  for (var k in obj) {
    keys.push(k);
  }
  keys.sort();
  return keys;
}

function validateAccessTokenScope() {
  // --- Self-constructed collections. No binding involved: this half is
  // directly comparable with the decision-node fixture.
  probe("new ArrayList", function () {
    var l = new java.util.ArrayList();
    l.add("a");
    return l.get(0) + ":" + l.size();
  });
  probe("new HashSet", function () {
    var s = new java.util.HashSet();
    s.add("a");
    return String(s.size());
  });
  probe("new LinkedHashSet", function () {
    var s = new java.util.LinkedHashSet();
    s.add("a");
    return String(s.size());
  });
  probe("new HashMap", function () {
    var m = new java.util.HashMap();
    m.put("a", 1);
    return String(m.size());
  });
  probe("ArrayList.iterator()", function () {
    var l = new java.util.ArrayList();
    l.add("a");
    return l.iterator().next();
  });
  probe("HashSet.iterator()", function () {
    var s = new java.util.HashSet();
    s.add("a");
    return s.iterator().next();
  });
  probe("LinkedHashSet.iterator()", function () {
    var s = new java.util.LinkedHashSet();
    s.add("a");
    return s.iterator().next();
  });
  probe("JSON.stringify(ArrayList)", function () {
    var l = new java.util.ArrayList();
    l.add("a");
    return JSON.stringify(l);
  });
  probe("getClass().getName()", function () {
    var l = new java.util.ArrayList();
    return l.getClass().getName();
  });

  // --- Is the DECLARED three-entry list the enforced one here, or is it
  // "no Java at all"? `java.lang.Object` and the two promise types are the
  // whole of `oauth2-validate-scope-next.json`'s allowLists; `java.lang.String`
  // is the negative control that is not on it.
  probe("typeof java", function () {
    return typeof java;
  });
  probe("typeof JavaImporter", function () {
    return typeof JavaImporter;
  });
  probe("new java.lang.Object [declared]", function () {
    var o = new java.lang.Object();
    return typeof o;
  });
  probe("new java.lang.String [not declared]", function () {
    return String(new java.lang.String("a"));
  });
  probe("org.forgerock.util.promise.PromiseImpl [declared]", function () {
    return typeof org.forgerock.util.promise.PromiseImpl;
  });
  // `java.lang.String` is NOT declared and yet resolves, so the enforced set is
  // neither the declared three nor the decision node's 51. These map the shape
  // of what it actually is.
  //
  // NOTE the probes below USE each class rather than `typeof` it. A blocked
  // name stays a `JavaPackage` object, so `typeof` answers "object" for a
  // blocked class AND for a live package — it cannot tell them apart, and an
  // earlier pass of this fixture read nine meaningless "object"s as evidence.
  // Only a construction or a call fails distinguishably.
  probe("java.lang.Math.max", function () {
    return java.lang.Math.max(1, 2);
  });
  probe("new java.lang.Integer", function () {
    return new java.lang.Integer(7).intValue();
  });
  probe("new java.lang.StringBuilder", function () {
    return new java.lang.StringBuilder("a").append("b").toString();
  });
  probe("new java.util.Date", function () {
    return typeof new java.util.Date().getTime();
  });
  probe("java.util.Collections.emptyList", function () {
    return java.util.Collections.emptyList().size();
  });
  probe("java.util.concurrent.TimeUnit.SECONDS", function () {
    return String(java.util.concurrent.TimeUnit.SECONDS);
  });
  probe("new java.text.SimpleDateFormat", function () {
    return new java.text.SimpleDateFormat("yyyy").toPattern();
  });
  probe("java.security.MessageDigest.getInstance", function () {
    return String(java.security.MessageDigest.getInstance("SHA-256").getDigestLength());
  });
  probe("new java.io.File (no I/O performed)", function () {
    return new java.io.File("probe").getName();
  });

  // --- requestedScopes: a Java collection AM handed us. Its contents are
  // scope names, so they are safe to log.
  probe("CONTROL requestedScopes.toArray", function () {
    return asArray(requestedScopes).join(",");
  });
  probe("requestedScopes.iterator()", function () {
    return requestedScopes.iterator().next();
  });
  probe("JSON.stringify(requestedScopes)", function () {
    return JSON.stringify(requestedScopes);
  });
  probe("String(requestedScopes)", function () {
    return String(requestedScopes);
  });

  // --- Is `requestedScopes` a JS array or a Java collection? The generated
  // type leaf declares it `any[]`, which would make `.length` correct and
  // `.toArray()` a type error — the exact opposite of the runtime if it is a
  // Java list. A wrong answer here is silent: `.length` on a Java list is
  // `undefined`, and `for (i < undefined)` simply never enters the loop.
  probe("Array.isArray(requestedScopes)", function () {
    return Array.isArray(requestedScopes);
  });
  probe("requestedScopes.length", function () {
    return typeof requestedScopes.length + ":" + requestedScopes.length;
  });
  probe("requestedScopes[0]", function () {
    return String(requestedScopes[0]);
  });
  probe("typeof requestedScopes.toArray", function () {
    return typeof requestedScopes.toArray;
  });
  probe("typeof requestedScopes.forEach", function () {
    return typeof requestedScopes.forEach;
  });
  probe("typeof requestedScopes.size", function () {
    return typeof requestedScopes.size;
  });

  // --- The two property bags. KEYS ONLY in the log; see the header.
  probe("keys(clientProperties)", function () {
    return keysOf(clientProperties).join(",");
  });
  probe("keys(requestProperties)", function () {
    return keysOf(requestProperties).join(",");
  });
  probe("JSON.stringify(clientProperties) [length only]", function () {
    return JSON.stringify(clientProperties).length + " chars";
  });
  probe("JSON.stringify(requestProperties) [length only]", function () {
    return JSON.stringify(requestProperties).length + " chars";
  });

  // Pass the request through unchanged: a 200 carrying both scopes is the
  // proof that every line above came from a run that actually happened.
  return asArray(requestedScopes);
}
