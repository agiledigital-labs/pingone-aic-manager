// Probe: Map.forEach on the next-gen requestParameters / requestHeaders bindings.
// AM's class shutter blocks keySet(), so scripts iterate with forEach. What does
// the callback receive (arity, key type, value type and shape), what does forEach
// return, does a throw inside the callback propagate, and what do bad arguments do?
//
// Safe to delete. Read only. Values are reported only for keys named
// `x-aic-probe*` / `probeq*`; other keys contribute a count, never a name or value.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}
function isProbeKey(key) {
  var k = String(key).toLowerCase();
  return k.indexOf("x-aic-probe") === 0 || k.indexOf("probeq") === 0;
}
function attempt(f) { try { return f(); } catch (e) { return "err:" + String(e); } }
function shape(v) {
  return {
    typeof: typeof v,
    isArray: attempt(function () { return Array.isArray(v); }),
    length: attempt(function () { return typeof v.length === "number" ? v.length : String(v.length); }),
    size: attempt(function () { return typeof v.size === "function" ? v.size() : "typeof " + typeof v.size; }),
    idx0: attempt(function () { return String(v[0]); }),
    get0: attempt(function () { return String(v.get(0)); }),
    str: attempt(function () { return String(v); }),
    typeofIdx0: attempt(function () { return typeof v[0]; }),
    jsMap: attempt(function () { return typeof v.map; }),
    jsJoin: attempt(function () { return typeof v.join; }),
    cls: attempt(function () { return String(v.getClass().getName()); })
  };
}
function probe(map) {
  var out = { typeofForEach: typeof map.forEach, calls: 0, arity: {}, keyTypes: {}, order: [], probes: {} };
  out.ret = attempt(function () {
    var r = map.forEach(function (k, v) {
      out.calls++;
      out.arity[arguments.length] = (out.arity[arguments.length] || 0) + 1;
      out.keyTypes[typeof k] = (out.keyTypes[typeof k] || 0) + 1;
      if (isProbeKey(k)) {
        out.order.push(String(k));
        out.probes[String(k)] = { value: shape(v), third: typeof arguments[2], thisType: typeof this, thisIsMap: this === map };
      }
    });
    return { typeof: typeof r, str: String(r) };
  });
  out.throwInside = attempt(function () {
    map.forEach(function () { throw new Error("probe-stop"); });
    return "no error";
  });
  out.throwInsideClass = attempt(function () {
    try { map.forEach(function () { throw new Error("probe-stop"); }); return "no error"; }
    catch (e) { return { name: String(e.name), message: String(e.message), isError: e instanceof Error, hasJava: typeof e.javaException }; }
  });
  out.noArgs = attempt(function () { map.forEach(); return "no error"; });
  out.nonFunction = attempt(function () { map.forEach(5); return "no error"; });
  out.oneArgFn = attempt(function () { var n = 0; map.forEach(function (k) { n++; }); return n; });
  out.forInCount = attempt(function () { var n = 0; for (var k in map) { n++; } return n; });
  return out;
}
try {
  emit({ ok: true, feature: "request-map-foreach", parameters: probe(requestParameters), headers: probe(requestHeaders) });
} catch (e) {
  emit({ ok: false, feature: "request-map-foreach", error: String(e) });
}
