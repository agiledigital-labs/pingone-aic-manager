// Probe: can a next-gen decision node get a URL's origin? Covers the WHATWG
// `URL` / `URLSearchParams` globals (browser/Node, not ES — Rhino need not have
// them), `java.net.URI` through the class shutter, and a pure-JS regex
// fallback. Each case is try/caught so one failure cannot mask the rest.
// Safe to delete.
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

var SAMPLE = "HTTPS://User:pw@Example.COM:443/a/b?q=1#frag";

// Candidate fallback: scheme + host[:port], userinfo stripped, lowercased,
// default port elided — the same normalisation WHATWG `origin` applies.
function originOf(url) {
  var m = /^([a-z][a-z0-9+.-]*):\/\/(?:[^@\/?#]*@)?([^\/?#:]+|\[[^\]]+\])(?::(\d+))?/i.exec(
    String(url)
  );
  if (!m) return null;
  var scheme = m[1].toLowerCase();
  var host = m[2].toLowerCase();
  var port = m[3];
  if ((scheme === "https" && port === "443") || (scheme === "http" && port === "80")) {
    port = undefined;
  }
  return scheme + "://" + host + (port ? ":" + port : "");
}

try {
  var results = [];
  results.push(probe("typeof URL", function () { return typeof URL; }));
  results.push(probe("typeof URLSearchParams", function () { return typeof URLSearchParams; }));
  // Control: a global that is definitely present.
  results.push(probe("typeof JSON", function () { return typeof JSON; }));
  results.push(probe("new URL(sample).origin", function () { return new URL(SAMPLE).origin; }));
  results.push(
    probe("java.net.URI getScheme/getHost/getPort", function () {
      var u = new java.net.URI(SAMPLE);
      return u.getScheme() + "|" + u.getHost() + "|" + u.getPort();
    })
  );
  results.push(
    probe("java.net.URL getProtocol/getHost", function () {
      var u = new java.net.URL(SAMPLE);
      return u.getProtocol() + "|" + u.getHost();
    })
  );
  results.push(probe("regex originOf(sample)", function () { return originOf(SAMPLE); }));
  results.push(
    probe("regex originOf(http://h:8080/x)", function () { return originOf("http://h:8080/x"); })
  );
  results.push(probe("regex originOf(relative)", function () { return originOf("/just/a/path"); }));
  emit({ ok: true, feature: "url-origin", value: results });
} catch (e) {
  emit({ ok: false, feature: "url-origin", error: String(e) });
}
