// Corpus copy of scripts/rhino-script-tester/fixtures/request-multivalue.script.js. Do not stub missing bindings.

// Probe: do requestHeaders / requestParameters values ever hold MORE THAN ONE
// element? Send the same header twice and the same query parameter twice, then
// report the element count each binding reports back.
//
// Safe to delete. Non-destructive (read only). Values are reported in full only
// for keys named `x-aic-probe*` / `probeq*`; every other key reports its element
// COUNT only, so a live request's cookies and tokens never reach the payload.
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

// Returns { n: <element count or null>, v: [values] | String(value) }.
function describe(value) {
  var out = { n: null, v: null };
  if (value === null || value === undefined) {
    out.v = String(value);
    return out;
  }
  var items = null;
  try {
    if (typeof value.size === "function" && typeof value.get === "function") {
      items = [];
      for (var i = 0; i < value.size(); i++) {
        items.push(String(value.get(i)));
      }
    } else if (typeof value.length === "number" && typeof value !== "string") {
      items = [];
      for (var j = 0; j < value.length; j++) {
        items.push(String(value[j]));
      }
    }
  } catch (e) {
    out.v = "err:" + String(e);
    return out;
  }
  if (items === null) {
    out.v = "scalar:" + typeof value;
    return out;
  }
  out.n = items.length;
  out.v = items;
  return out;
}

function survey(map, label) {
  var result = { label: label, className: null, counts: {}, probes: {}, error: null };
  if (map === null || map === undefined) {
    result.error = String(map);
    return result;
  }
  try {
    result.className = String(map.getClass().getName());
  } catch (ce) {
    result.className = "err:" + String(ce);
  }
  try {
    for (var key in map) {
      if (typeof map[key] === "function") {
        continue;
      }
      var d = describe(map[key]);
      result.counts[String(key)] = d.n;
      if (isProbeKey(key)) {
        result.probes[String(key)] = d.v;
      }
    }
  } catch (e) {
    result.error = String(e);
  }
  // Also ask by name, in case iteration and .get() disagree.
  var byName = ["x-aic-probe", "X-Aic-Probe", "probeq", "x-aic-probe-joined", "probeqjoined"];
  for (var b = 0; b < byName.length; b++) {
    try {
      result.probes["get:" + byName[b]] = describe(map.get(byName[b])).v;
    } catch (ge) {
      result.probes["get:" + byName[b]] = "err:" + String(ge);
    }
  }
  return result;
}

try {
  emit({
    ok: true,
    feature: "request-multivalue",
    headers: survey(requestHeaders, "requestHeaders"),
    parameters: survey(requestParameters, "requestParameters"),
  });
} catch (e) {
  emit({ ok: false, feature: "request-multivalue", error: String(e) });
}
