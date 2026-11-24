#!/usr/bin/env bash
# experiment-journey-httpclient-header.sh
#
# Question: which JS values does a next-gen journey decision script's
# `httpClient.send(url, {headers: {K: v}})` accept as a header value, and what does the tenant
# put on the wire for the ones it accepts? The known case: `"Bearer " + token` is refused with
# "Cannot add values for key Authorization" while a literal, `.toString()` and `.concat` pass.
# Hypothesis (a guess, to be confirmed or killed by the table): the refused values are Rhino
# `ConsString`s (what `+` builds from non-constant operands), not `java.lang.String`.
#
# Method: the docs/api/12 echo (httpbin.org/headers returns the headers it received). Each case
# is its own tree with one scripted node making one send under try/catch, so a failure in one
# case cannot hide the others. Each case reports `typeof`, the Java class of the value when it
# is a Java object (via `Packages`-free probing: `String(v.constructor)`), the caught error text,
# the status and the echoed header. Positive control: a literal must arrive.
#
# Objects are kdemo-g2-*, deleted on exit, deletion confirmed by a GET.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g2}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import BASE, Fixture, authenticate, log, SUCCESS, hidden

# The tenant's own token endpoint, with a client that does not exist: the answer is an error
# either way, and its status says how the endpoint read the body.
TOKEN = BASE + "/am/oauth2/realms/root/realms/alpha/access_token"

fx = Fixture("hdr")

# name -> JS statements leaving the header value in `v`. `t` is a non-constant string.
HEADER_CASES = {
    "literal": 'v = "abc";',
    "concat-var": 'v = "Bearer " + t;',
    "concat-literals": 'v = "Bearer " + "abc";',
    "plus-empty": 'v = "" + t;',
    "plus-eq": 'v = "Bearer "; v += t;',
    "tostring": 'v = ("Bearer " + t).toString();',
    "concat-method": 'v = "Bearer ".concat(t);',
    "string-fn": 'v = String("Bearer " + t);',
    "new-String": 'v = new String("Bearer " + t);',
    "slice": 'v = ("Bearer " + t).slice(0);',
    "join": 'v = ["Bearer ", t].join("");',
    "replace": 'v = ("Bearer " + t).replace("x", "y");',
    "json-parse": 'v = JSON.parse(\'{"a":"abc"}\').a;',
    "json-parse-concat": 'v = "Bearer " + JSON.parse(\'{"a":"abc"}\').a;',
    "number": 'v = 1;',
    "number-concat": 'v = "n" + 1;',
    "boolean": 'v = true;',
    "null": 'v = null;',
    "undefined": 'v = undefined;',
    "array": 'v = ["a", "b"];',
    "object": 'v = {a: 1};',
    "template": 'v = eval("`Bearer ${t}`");',
    "java-string": 'v = String(java.lang.String.valueOf("Bearer " + t));',
    "number-float": 'v = 1.5;',
    "number-big": 'v = 1e21;',
    "number-zero": 'v = 0;',
    "string-object-wrapper-literal": 'v = new String("abc");',
    "array-nested": 'v = [["a"]];',
    "array-one": 'v = ["a"];',
    "array-empty": 'v = [];',
    "array-number": 'v = [1];',
    "array-cons": 'v = ["Bearer " + t];',
    "array-mixed": 'v = ["a", "Bearer " + t];',
}

# name -> JS statements leaving the whole request in `u` (url) and `o` (options): the other
# places a ConsString can reach `httpClient.send`. Echo is httpbin.org/post.
REQUEST_CASES = {
    "url-cons": 'u = "https://httpbin.org/" + (t === "abc" ? "post" : "get"); o = {method: "POST"};',
    "key-cons": 'u = "https://httpbin.org/post"; var k = "X-" + t; var h = {}; h[k] = "v"; o = {method: "POST", headers: h};',
    "method-cons": 'u = "https://httpbin.org/post"; o = {method: "PO" + t.slice(0, 0) + "ST"};',
    "body-string-literal": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded"}, body: "a=1&b=x"};',
    "body-string-cons": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded"}, body: "a=1&b=" + t};',
    "form-literal": 'u = "https://httpbin.org/post"; o = {method: "POST", form: {a: "1", b: "x"}};',
    "form-cons": 'u = "https://httpbin.org/post"; o = {method: "POST", form: {a: "1", b: "x" + t}};',
    "body-object-cons": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/json"}, body: {a: "x" + t}};',
    "auth-cons": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Authorization": "Bearer " + t}};',
    "auth-tostring": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Authorization": ("Bearer " + t).toString()}};',
    "key-cons-echo": 'u = "https://httpbin.org/post"; var k = "X-" + t; var h = {}; h[k] = "v"; o = {method: "POST", headers: h};',
    "body-cons-noheader": 'u = "https://httpbin.org/post"; o = {method: "POST", body: "a=1&b=" + t};',
    "body-literal-noheader": 'u = "https://httpbin.org/post"; o = {method: "POST", body: "a=1&b=x"};',
    "body-cons-tostring": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded"}, body: ("a=1&b=" + t).toString()};',
    "body-cons-textplain": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "text/plain"}, body: "a=1&b=" + t};',
    "body-literal-textplain": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "text/plain"}, body: "a=1&b=x"};',
    "body-cons-json": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/json"}, body: "a=1&b=" + t};',
    "body-literal-json": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/json"}, body: "a=1&b=x"};',
    "body-stringify-json": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify({a: 1})};',
    "body-cons-special": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded"}, body: "q=\\"" + t + "\\\\\\u00e9"};',
    "token-literal": 'u = "TOKEN"; o = {method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded"}, body: "grant_type=client_credentials&client_id=kdemo-g2-none"};',
    "token-cons": 'u = "TOKEN"; o = {method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded"}, body: "grant_type=client_credentials&client_id=kdemo-g2-" + t.slice(0, 0) + "none"};',
    "token-form": 'u = "TOKEN"; o = {method: "POST", form: {grant_type: "client_credentials", client_id: "kdemo-g2-none"}};',
    "ctype-cons": 'u = "https://httpbin.org/post"; o = {method: "POST", headers: {"Content-Type": "application/x-www-form-urlencoded" + t.slice(0, 0) + ""}, body: "a=1"};',
}

SRC = r"""
if (callbacks.isEmpty()) {
  var out = {};
  var t = String(nodeState.get("realm")).length > 100 ? "x" : "abc";
  var v, u = "https://httpbin.org/headers", o;
  try { BUILD } catch (e) { out.buildError = String(e).slice(0, 300); }
  if (!("buildError" in out)) {
    if (o === undefined) { o = { method: "GET", headers: {"X-Probe": v} }; }
    out.typeof = typeof v;
    try { out.ctor = v === null || v === undefined ? "-" : String(v.constructor).slice(0, 60); } catch (e) { out.ctor = "?"; }
    try {
      var resp = null;
      for (var attempt = 0; attempt < 5; attempt++) {
        var fut = httpClient.send(u, o);
        out.sentOk = true;
        resp = fut.get();
        if (resp.status !== 429) { break; }
        var until = Date.now() + 2000;
        while (Date.now() < until) { /* httpbin rate-limits bursts */ }
      }
      out.status = resp.status;
      var p;
      try { p = JSON.parse(resp.text()); } catch (e) { p = {}; }
      out.echoed = p.headers ? (p.headers["X-Probe"] === undefined ? "<absent>" : p.headers["X-Probe"]) : "?";
      if (p.data !== undefined) { out.data = p.data; }
      if (p.form !== undefined) { out.form = JSON.stringify(p.form); }
      if (p.headers) { out.auth = p.headers["Authorization"]; out.xabc = p.headers["X-Abc"]; out.ctype = p.headers["Content-Type"] || "<none>"; out.len = p.headers["Content-Length"] || "<none>"; }
      out.url = p.url;
      if (p.url === undefined) { out.text = resp.text().slice(0, 200); }
    } catch (e) {
      out.sendError = String(e).slice(0, 400);
    }
  }
  callbacksBuilder.hiddenValueCallback("r", JSON.stringify(out));
} else {
  action.goTo("true");
}
"""

try:
    for name, build in {**HEADER_CASES, **REQUEST_CASES}.items():
        build = build.replace("TOKEN", TOKEN)
        s = fx.scripted(name, SRC.replace("BUILD", build), ["true"])
        tree = fx.tree(name, s, {s: ("ScriptedDecisionNode", name, {"true": SUCCESS})})
        st, body, _ = authenticate(tree)
        v = hidden(body, "r") if isinstance(body, dict) else None
        log(f"=== {name}  [{build}]  authenticate HTTP {st}")
        if v is None:
            shown = {k: body.get(k) for k in ("code", "reason", "message")} if isinstance(body, dict) else str(body)[:300]
            log("    journey error:", json.dumps(shown))
            continue
        r = json.loads(v)
        for k in ("buildError", "typeof", "ctor", "sendError", "sentOk", "status", "echoed", "data", "form", "ctype", "len", "auth", "xabc", "text"):
            if k in r:
                log(f"    {k}: {r[k]}")
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
