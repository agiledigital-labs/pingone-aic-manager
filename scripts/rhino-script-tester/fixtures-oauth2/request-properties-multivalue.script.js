// Probe: can `requestProperties.requestHeaders` / `.requestParams` hold more
// than one element per key, the way the scripted-decision bindings can?
//
// PASSTHROUGH — returns the requested scopes unchanged, so the token request
// succeeds and the run also serves as a control that the wiring works. The
// answer is the LOG LINE, not the response: this context has no callback to
// emit a payload through. Read it with `aic logs tx <txid>`.
//
// Values are logged in full only for keys named `x-aic-probe*` / `probeq*`;
// every other key contributes its element COUNT only, so the client's
// `Authorization` / `client_secret` never reach the log.
function asArray(collection) {
  var out = [];
  var items = collection.toArray();
  for (var i = 0; i < items.length; i++) {
    out.push(String(items[i]));
  }
  return out;
}

function isProbeKey(key) {
  var k = String(key).toLowerCase();
  return k.indexOf("x-aic-probe") === 0 || k.indexOf("probeq") === 0;
}

// A Java list here, a bare string there — report which, and how many.
function describe(value) {
  if (value === null || value === undefined) {
    return { n: -1, v: String(value) };
  }
  if (typeof value.size === "function" && typeof value.get === "function") {
    var items = [];
    for (var i = 0; i < value.size(); i++) {
      items.push(String(value.get(i)));
    }
    return { n: items.length, v: items };
  }
  return { n: -2, v: String(value) };
}

function survey(map) {
  var out = { counts: {}, probes: {}, error: null };
  if (map === null || map === undefined) {
    out.error = String(map);
    return out;
  }
  try {
    for (var k in map) {
      if (typeof map[k] === "function") {
        continue;
      }
      var d = describe(map[k]);
      out.counts[String(k)] = d.n;
      if (isProbeKey(k)) {
        out.probes[String(k)] = d.v;
      }
    }
  } catch (e) {
    out.error = String(e);
  }
  var byName = ["x-aic-probe", "X-Aic-Probe", "probeq", "scope"];
  for (var b = 0; b < byName.length; b++) {
    try {
      out.probes["get:" + byName[b]] = describe(map[byName[b]]).v;
    } catch (ge) {
      out.probes["get:" + byName[b]] = "err:" + String(ge);
    }
  }
  return out;
}

function validateAccessTokenScope() {
  var requested = asArray(requestedScopes);
  var report = {
    requested: requested,
    headers: null,
    params: null,
    error: null,
  };
  try {
    report.headers = survey(requestProperties.requestHeaders);
    report.params = survey(requestProperties.requestParams);
  } catch (e) {
    report.error = String(e);
  }
  logger.error("AICPROBE-MV " + JSON.stringify(report));
  return requested;
}
