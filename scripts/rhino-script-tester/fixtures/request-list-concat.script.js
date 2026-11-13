// Probe: Array.prototype.concat, String() and JSON.stringify on the Java value
// lists next-gen AM hands out from requestParameters.get(k) / requestHeaders.get(k).
// Run with run-journey-multivalue.sh-style duplicated probe headers/params plus
// `probeqempty=` and an empty `X-Aic-Probe-Empty` header.
//
// Safe to delete. Read only. Only keys named `x-aic-probe*` / `probeq*` are read;
// no map is iterated or serialised.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}
function attempt(f) { try { return f(); } catch (e) { return "err:" + String(e); } }
function result(r) {
  return {
    typeof: typeof r,
    isArray: attempt(function () { return Array.isArray(r); }),
    length: attempt(function () { return r.length; }),
    items: attempt(function () {
      var out = [];
      for (var i = 0; i < r.length; i++) { out.push({ typeof: typeof r[i], value: r[i] === null ? null : String(r[i]), isList: r[i] !== null && typeof r[i] === "object" }); }
      return out;
    }),
    eqAlpha: attempt(function () { return r[0] === "alpha"; }),
    join: attempt(function () { return r.join("|"); }),
    str: attempt(function () { return String(r); }),
    json: attempt(function () { return JSON.stringify(r); })
  };
}
function lists(map, names) {
  var out = {};
  for (var i = 0; i < names.length; i++) { out[names[i]] = map.get(names[i]); }
  return out;
}
function probe(map, names) {
  var l = lists(map, names);
  var many = l[names[0]], single = l[names[1]], joined = l[names[2]], empty = l[names[3]], missing = l[names[4]];
  return {
    got: {
      many: attempt(function () { return { typeof: typeof many, isNull: many === null, size: many === null ? null : many.size() }; }),
      empty: attempt(function () { return { typeof: typeof empty, isNull: empty === null, size: empty === null || empty === undefined ? null : empty.size(), str: String(empty) }; }),
      missing: attempt(function () { return { typeof: typeof missing, isNull: missing === null }; })
    },
    concatMany: attempt(function () { return result([].concat(many)); }),
    concatSingle: attempt(function () { return result([].concat(single)); }),
    concatJoined: attempt(function () { return result([].concat(joined)); }),
    concatEmpty: attempt(function () { return result([].concat(empty)); }),
    concatMissing: attempt(function () { return result([].concat(missing)); }),
    concatTwo: attempt(function () { return result([].concat(many, single)); }),
    concatMixed: attempt(function () { return result([].concat("x", many, 1, single, ["y"])); }),
    concatOntoArray: attempt(function () { return result(["first"].concat(many)); }),
    concatCallOnList: attempt(function () { return result(Array.prototype.concat.call(many, "z")); }),
    typeofConcatMember: attempt(function () { return typeof many.concat; }),
    strMany: attempt(function () { return String(many); }),
    strEmpty: attempt(function () { return String(empty); }),
    jsonMany: attempt(function () { return JSON.stringify(many); }),
    jsonSingle: attempt(function () { return JSON.stringify(single); }),
    jsonEmpty: attempt(function () { return JSON.stringify(empty); }),
    jsonNested: attempt(function () { return JSON.stringify({ k: many }); }),
    jsonInArray: attempt(function () { return JSON.stringify([many]); }),
    jsonFromToJSON: attempt(function () { return JSON.stringify({ toJSON: function () { return many; } }); }),
    jsonFromGetter: attempt(function () { var o = {}; Object.defineProperty(o, "v", { enumerable: true, get: function () { return many; } }); return JSON.stringify(o); }),
    jsonFromReplacer: attempt(function () { return JSON.stringify({ v: 1 }, function (k, v) { return k === "v" ? many : v; }); }),
    jsonReplacerSees: attempt(function () {
      var seen = [];
      var out = JSON.stringify({ v: many }, function (k, v) { if (k === "v") { seen.push(Array.isArray(v), typeof v.size, v === many); } return v; });
      return { out: out, seen: seen };
    }),
    jsonListedDirect: attempt(function () { return JSON.stringify(many, ["0"]); }),
    jsonListedNested: attempt(function () { return JSON.stringify({ v: many, w: 1 }, ["v"]); }),
    jsonListedGetter: attempt(function () { var o = {}; Object.defineProperty(o, "v", { enumerable: true, get: function () { return many; } }); return JSON.stringify(o, ["v"]); }),
    jsonIndented: attempt(function () { return JSON.stringify({ v: many }, null, 1); })
  };
}
try {
  emit({
    ok: true,
    feature: "request-list-concat",
    parameters: probe(requestParameters, ["probeq", "probeqsingle", "probeqjoined", "probeqempty", "probeqmissing"]),
    headers: probe(requestHeaders, ["x-aic-probe", "x-aic-probe-single", "x-aic-probe-joined", "x-aic-probe-empty", "x-aic-probe-missing"]),
    cross: attempt(function () { return result([].concat(requestParameters.get("probeq"), requestHeaders.get("x-aic-probe"))); })
  });
} catch (e) {
  emit({ ok: false, feature: "request-list-concat", error: String(e) });
}
