#!/usr/bin/env bash
# experiment-journey-nonjson-body.sh
#
# Question: what does `httpClient.send` (next-gen journey decision script,
# Content-Type application/json, an OBJECT `body` — the form the rhino-local
# emulator models) put on the wire when a body value is not JSON data?
#   Date, NaN, 1/0, -1/0, -0, a function, a cycle (o.self = o), a RegExp, and
#   a java.util.Date (if the class shutter lets the script construct one).
#
# Method: the docs/api/12 echo (httpbin.org/post returns the raw body it got as
# `data`). Each case is its own tree with one scripted node making one send, so
# an uncatchable failure in one case (a journey error) cannot hide the others.
# Inside the node the send is wrapped in try/catch; a caught error is reported
# with its message. Positive control: {"a":"x"} must arrive as exactly that.
# Every value sits under key `v` beside a sentinel `a:"x"`, so a dropped key is
# visible. The script's own JSON.stringify of the body (or its error) is
# reported alongside.
#
# Every tree, node and script is zzemu-*, deleted on exit, deletion confirmed
# by a GET.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import base64
import json
from experiment_journey_lib import Fixture, authenticate, log, SUCCESS, hidden

fx = Fixture("nonjson")

# name -> JS expression building the body (`b`).
CASES = {
    "control": 'b = {a: "x"};',
    "date": 'b = {a: "x", v: new Date(0)};',
    "nan": 'b = {a: "x", v: NaN};',
    "posinf": 'b = {a: "x", v: 1/0};',
    "neginf": 'b = {a: "x", v: -1/0};',
    "negzero": 'b = {a: "x", v: -0};',
    "function": 'b = {a: "x", v: function () { return 1; }};',
    "cycle": 'b = {a: "x"}; b.self = b;',
    "regexp": 'b = {a: "x", v: /a/g};',
    "javadate": 'b = {a: "x", v: new java.util.Date(0)};',
}

SRC = r"""
if (callbacks.isEmpty()) {
  var out = {};
  var b;
  try { BUILD } catch (e) { out.buildError = String(e).slice(0, 300); }
  if (!("buildError" in out)) {
    try { out.local = JSON.stringify(b); } catch (e) { out.localError = String(e).slice(0, 200); }
    try {
      var resp = null;
      for (var attempt = 0; attempt < 5; attempt++) {
        resp = httpClient.send("https://httpbin.org/post", { method: "POST",
          headers: { "Content-Type": "application/json" }, body: b }).get();
        if (resp.status !== 429) { break; }
        var until = Date.now() + 2000;
        while (Date.now() < until) { /* httpbin rate-limits bursts */ }
      }
      out.attempts = attempt + 1;
      out.status = resp.status;
      var p = JSON.parse(resp.text());
      out.data = p.data;
      out.len = p.headers ? (p.headers["Content-Length"] || "<none>") : "?";
      out.ctype = p.headers ? (p.headers["Content-Type"] || "<none>") : "?";
    } catch (e) {
      out.sendError = String(e).slice(0, 400);
    }
  }
  callbacksBuilder.hiddenValueCallback("r", JSON.stringify(out));
} else {
  action.goTo("true");
}
"""


def wire(d):
    pre = "data:application/octet-stream;base64,"
    if isinstance(d, str) and d.startswith(pre):
        return base64.b64decode(d[len(pre):]).decode("utf-8", "replace") + "  (base64 data: URL)"
    return d


try:
    for name, build in CASES.items():
        s = fx.scripted(name, SRC.replace("BUILD", build), ["true"])
        tree = fx.tree(name, s, {s: ("ScriptedDecisionNode", name, {"true": SUCCESS})})
        st, body, _ = authenticate(tree)
        v = hidden(body, "r") if isinstance(body, dict) else None
        log(f"=== {name}  [{build}]  authenticate HTTP {st}")
        if v is None:
            # journey error: show only code/reason/message, never the whole body
            shown = {k: body.get(k) for k in ("code", "reason", "message")} if isinstance(body, dict) else str(body)[:300]
            log("    journey error:", json.dumps(shown))
            continue
        r = json.loads(v)
        for k in ("buildError", "local", "localError", "sendError", "status", "attempts", "len", "ctype"):
            if k in r:
                log(f"    {k}: {r[k]}")
        if "data" in r:
            log(f"    wire: {wire(r['data'])}")
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
