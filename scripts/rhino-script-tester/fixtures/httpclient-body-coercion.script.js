// Probe: how httpClient.send serializes a JS object body on the next-gen engine.
// Safe to delete.
//
// Two specific claims are under test:
//   1. `undefined` property values are converted to `null` (rather than being
//      omitted, which is what JSON.stringify does).
//   2. Whole numbers are rendered as doubles — `1` goes out as `1.0`, which
//      breaks receivers that validate an integer type. The proposed workaround is
//      `new java.lang.Integer(1)`.
//
// Method: send one object to a public echo service and compare the raw body it
// received against `JSON.stringify` of the SAME object computed locally. The diff
// between the two isolates httpClient's serializer from JS-level rendering.
//
// The echo target is httpbin.org (public request-reflector) and the payload is
// synthetic scalars only — no tenant data, no secrets, nothing derived from
// nodeState. If egress is blocked the fixture reports the transport error and the
// java.lang.* constructibility rows below are still meaningful.
var ECHO_URL = "https://httpbin.org/post";

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

  // java.lang boxed numerics. NOTE: the next-gen allow-list in
  // docs/api/bindings/scripted-decision-next.json contains Byte, Short, Long,
  // Float, Number and Void but NOT Integer — so whether the documented
  // `new java.lang.Integer(1)` workaround is even constructible is itself a
  // question, and a second data point on allow-list-vs-enforcement.
  results.push(
    probe("new java.lang.Integer(1)", function () {
      return String(new java.lang.Integer(1));
    })
  );
  results.push(
    probe("new java.lang.Long(1)", function () {
      return String(new java.lang.Long(1));
    })
  );
  results.push(
    probe("new java.lang.Short(1)", function () {
      return String(new java.lang.Short(1));
    })
  );
  results.push(
    probe("new java.lang.Double(1)", function () {
      return String(new java.lang.Double(1));
    })
  );
  results.push(
    probe("java.lang.Integer.valueOf(1)", function () {
      return String(java.lang.Integer.valueOf(1));
    })
  );

  // The body under test. Built in two halves so a blocked java.lang.Integer
  // cannot prevent the plain-JS rows from being observed.
  var body = {
    intOne: 1,
    intZero: 0,
    negInt: -5,
    bigInt: 1000000,
    floatVal: 1.5,
    undefField: undefined,
    nullField: null,
    strField: "s",
    boolField: true,
    nested: { u: undefined, n: null, i: 2 },
    arr: [1, undefined, 3],
  };
  var javaIntAdded = false;
  try {
    body.javaInt = new java.lang.Integer(1);
    body.javaLong = new java.lang.Long(1);
    javaIntAdded = true;
  } catch (e) {
    results.push({
      name: "add java boxed ints to body",
      ok: false,
      error: String(e),
    });
  }

  // What JS itself renders, for side-by-side comparison with the wire body.
  var localStringify = JSON.stringify(body);

  var echo = probe("httpClient.send echo", function () {
    var response = httpClient
      .send(ECHO_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: body,
      })
      .get();
    if (!response.ok) {
      return "HTTP " + response.status + " " + response.statusText;
    }
    // httpbin returns { data: "<raw request body>", json: {...} }. `data` is the
    // byte-level answer we are after.
    var parsed = JSON.parse(response.text());
    return parsed.data;
  });
  results.push(echo);

  emit({
    ok: true,
    feature: "httpclient-body-coercion",
    javaIntAdded: javaIntAdded,
    localStringify: localStringify,
    value: results,
  });
} catch (e) {
  emit({ ok: false, feature: "httpclient-body-coercion", error: String(e) });
}
