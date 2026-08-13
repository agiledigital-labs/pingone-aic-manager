// Probe: dump requestHeaders / requestParameters / requestCookies as seen by
// a next-gen scripted decision. Safe to delete. Non-destructive (read only).
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function stringifyValue(v) {
  if (v === null || v === undefined) {
    return v;
  }
  try {
    if (typeof v.toArray === "function") {
      var arr = v.toArray();
      var out = [];
      for (var i = 0; i < arr.length; i++) {
        out.push(String(arr[i]));
      }
      return out;
    }
  } catch (_e) {}
  if (typeof v.size === "function" && typeof v.get === "function") {
    try {
      var list = [];
      for (var j = 0; j < v.size(); j++) {
        list.push(String(v.get(j)));
      }
      return list;
    } catch (_e2) {}
  }
  return String(v);
}

function dumpMap(m, extraCandidates) {
  var result = {
    typeofValue: typeof m,
    className: null,
    stringified: null,
    keys: [],
    values: {},
    known: {},
    methods: [],
    enumerateError: null,
  };
  if (m === null || m === undefined) {
    result.typeofValue = String(m);
    return result;
  }
  try {
    result.stringified = String(m);
  } catch (se) {
    result.stringified = "err:" + String(se);
  }
  try {
    if (m.getClass) {
      result.className = String(m.getClass().getName());
    }
  } catch (ce) {
    result.className = "err:" + String(ce);
  }
  try {
    for (var methodName in m) {
      result.methods.push(String(methodName));
    }
    result.methods.sort();
  } catch (me) {
    result.methods = ["err:" + String(me)];
  }
  try {
    for (var fk in m) {
      if (typeof m[fk] === "function") {
        continue;
      }
      result.keys.push(String(fk));
      try {
        result.values[String(fk)] = stringifyValue(m[fk]);
      } catch (inner) {
        result.values[String(fk)] = "err:" + String(inner);
      }
    }
    result.keys.sort();
  } catch (e) {
    result.enumerateError = String(e);
  }
  var candidates = [
    "origin",
    "Origin",
    "ORIGIN",
    "referer",
    "Referer",
    "referrer",
    "host",
    "Host",
    "x-forwarded-host",
    "X-Forwarded-Host",
    "x-forwarded-proto",
    "x-forwarded-for",
    "x-forwarded-prefix",
    "user-agent",
    "User-Agent",
    "accept",
    "content-type",
    "Content-Type",
    "cookie",
    "accept-api-version",
    "Accept-API-Version",
    "x-requested-with",
    "X-Requested-With",
    "referrer-policy",
    "authorization",
    "Authorization",
  ];
  if (extraCandidates) {
    for (var x = 0; x < extraCandidates.length; x++) {
      candidates.push(extraCandidates[x]);
    }
  }
  for (var c = 0; c < candidates.length; c++) {
    var name = candidates[c];
    try {
      result.known[name] = {
        containsKey: typeof m.containsKey === "function" ? !!m.containsKey(name) : null,
        get: stringifyValue(m.get(name)),
      };
    } catch (ge) {
      result.known[name] = { error: String(ge) };
    }
  }
  return result;
}

try {
  emit({
    ok: true,
    feature: "request-headers-dump",
    headers: dumpMap(requestHeaders),
    parameters: dumpMap(requestParameters),
    cookies: dumpMap(requestCookies),
  });
} catch (e) {
  emit({ ok: false, feature: "request-headers-dump", error: String(e) });
}
