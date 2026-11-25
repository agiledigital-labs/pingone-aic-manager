#!/usr/bin/env bash
# experiment-journey-http-failure.sh
#
# Question: what does a next-gen scripted decision see when `httpClient.send(...)` cannot reach its
# host (connection refused, an unresolvable name, a non-routable address)? Where does it throw
# (`send` or `.get()`), and what do the thrown value's name, message, class and text look like?
#
# Method: one script per target. It wraps `send` and `.get()` separately in try/catch and reports
# every observable of the caught value in a hidden callback. Objects are kdemo-g3-*, deleted on
# exit, deletion confirmed by a GET. The targets are loopback port 1, a `.invalid` name and an
# RFC 5737 documentation address; nothing real is contacted.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g3}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, log, safe, cbs, SUCCESS)

fx = Fixture("httpf")

SRC = r'''
function look(e) {
  var o = {};
  try { o.typeof = typeof e; } catch (x) {}
  try { o.string = String(e); } catch (x) { o.string = "<throws>"; }
  try { o.name = e.name; } catch (x) {}
  try { o.message = String(e.message); } catch (x) {}
  try { o.isError = e instanceof Error; } catch (x) {}
  try { o.ctor = e.constructor ? String(e.constructor.name) : null; } catch (x) {}
  try { o.keys = Object.getOwnPropertyNames(e).join(","); } catch (x) {}
  try { o.javaException = e.javaException ? String(e.javaException.getClass().getName()) : null; } catch (x) { o.javaException = "<throws>"; }
  try { o.cause = e.javaException && e.javaException.getCause() ? String(e.javaException.getCause().getClass().getName()) + ": " + String(e.javaException.getCause().getMessage()) : null; } catch (x) { o.cause = "<throws>"; }
  try { o.stack = e.stack ? String(e.stack).split("\n")[0] : null; } catch (x) {}
  return o;
}
var out = { where: "none" };
var pending = null;
try { pending = httpClient.send("TARGET", { method: "GET" }); out.sendReturned = typeof pending; }
catch (e) { out.where = "send"; out.err = look(e); }
if (pending) {
  try { var r = pending.get(); out.got = typeof r; out.status = r && r.status; }
  catch (e) { out.where = "get"; out.err = look(e); }
}
callbacksBuilder.hiddenValueCallback("res", JSON.stringify(out));
action.goTo("true");
'''


def probe(tag, target):
    s = fx.scripted(tag, SRC.replace("TARGET", target), ["true"])
    tree = fx.tree(tag, s, {s: ("ScriptedDecisionNode", tag, {"true": SUCCESS})})
    log(f"=== {tag}: {target}")
    st, body, _ = authenticate(tree)
    log(f"    HTTP {st}")
    if isinstance(body, dict) and "callbacks" in body:
        for c in cbs(body):
            if c["type"] == "HiddenValueCallback":
                log("   ", str(c["output"].get("value"))[:1500])
    else:
        log("   ", json.dumps(safe(body))[:300])


try:
    probe("refused", "http://127.0.0.1:1/")
    probe("dns", "http://kdemo-g3-nonexistent.invalid/")
    probe("blackhole", "http://192.0.2.1:81/")
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        if "delete=404" not in line:
            log("   ", line)
PY
