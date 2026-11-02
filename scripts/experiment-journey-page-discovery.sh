#!/usr/bin/env bash
# experiment-journey-page-discovery.sh
#
# Question this answers:
#   When a journey first reaches a PageNode (the POST that returns the page's
#   callbacks), does AM execute a next-gen ScriptedDecisionNode child that
#   queues no callbacks? experiment-journey-page.sh measured that child with
#   nodeState counters, which a discarded discovery run would not leave behind,
#   so it could not tell "not run" from "run and its state thrown away".
#
# Method:
#   The side-effect channel is external: every execution of a probe child calls
#   openidm.create("managed/alpha_user", <fresh UUID>, ...) with a userName
#   prefixed by the variant, and a `description` recording whether `callbacks`
#   was empty and the zzemu counter it read from nodeState. The records are
#   counted through IDM REST before anything runs (baseline, must be 0), after
#   the first POST (page returned, nothing submitted), and after the page is
#   submitted. Variants:
#     nocb-last   [UsernameCollector, callback-free script]
#     nocb-first  [callback-free script, UsernameCollector]
#     cb-last     positive control: the script queues a NameCallback when
#                 `callbacks` is empty and creates on every execution, so it
#                 must run at discovery; also shows whether it runs again on
#                 submit
#     throw-last  [UsernameCollector, script that throws]: a second channel
#                 independent of IDM; a discovery run would fail the first POST
#     goto-last   [UsernameCollector, callback-free script calling goTo("b")
#                 when it has never run before]: whether a discovery-time goTo
#                 can end the page before it is shown
#   Page outcomes are wired to a dump node that never completes, so no session
#   is issued.
#
# Every tree, node and script is named zzemu-*; every record the probes create
# has a UUID _id and a zzemu-disc-* userName. All are deleted on exit and the
# deletion is confirmed by a re-read and a final zzemu-disc-* query. Output
# passes through check-sensitive-metadata.sh --redact.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
import time
import urllib.parse
from experiment_journey_lib import (Fixture, authenticate, show, fill, dump_source,
                                    log, call, REALM, FAILURE, list_outcomes)

# A JS UUIDv4 per execution, so each run creates a distinct record.
CREATE = r"""
function zzUuid() {
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, function (c) {
    var r = Math.floor(Math.random() * 16);
    return (c === "x" ? r : (r & 3) | 8).toString(16);
  });
}
var zzPrev = nodeState.get("zzDiscRuns");
var zzRuns = zzPrev === null ? 1 : Number(zzPrev) + 1;
nodeState.putShared("zzDiscRuns", String(zzRuns));
var zzId = zzUuid();
openidm.create("managed/alpha_user", zzId, {
  userName: PREFIX + zzId.substring(0, 8),
  givenName: "zzemu", sn: "zzemu", mail: "zzemu-disc@example.com",
  description: "empty=" + callbacks.isEmpty() + " runs=" + zzRuns
});
"""

BODIES = {
    "nocb": CREATE + 'action.goTo("true");',
    "cb": CREATE + r"""
if (callbacks.isEmpty()) {
  callbacksBuilder.nameCallback("zzemu script prompt");
} else {
  action.goTo("true");
}""",
    "throw": 'throw new Error("zzemu discovery probe");',
    "goto": r"""
if (nodeState.get("zzGotoSeen") === null) {
  nodeState.putShared("zzGotoSeen", "1");
  action.goTo("b");
} else {
  action.goTo("a");
}""",
}

KEYS = ["username", "zzDiscRuns", "zzGotoSeen"]


def count(prefix):
    q = urllib.parse.quote(f'userName sw "{prefix}"')
    st, b, _ = call("GET", f"/openidm/managed/{REALM}_user?_queryFilter={q}"
                    "&_fields=_id,userName,description", api="resource=1.0")
    if st != 200:
        raise RuntimeError(f"query {prefix} -> {st}: {b}")
    return b.get("result", [])


def report(label, prefix):
    time.sleep(1)
    rs = count(prefix)
    log(f"    COUNT {label}: {len(rs)} {sorted(r.get('description') for r in rs)}")
    return rs


def build(fx, tag, kind, first):
    prefix = f"zzemu-disc-{fx.run}-{tag}-"
    outcomes = ["a", "b"] if kind == "goto" else ["true"]
    s = fx.scripted(f"{tag}-child", BODIES[kind].replace("PREFIX", json.dumps(prefix)), outcomes)
    u = fx.node("UsernameCollectorNode", {})
    kids = [{"_id": u, "nodeType": "UsernameCollectorNode", "displayName": "zzemu user"},
            {"_id": s, "nodeType": "ScriptedDecisionNode", "displayName": "zzemu script"}]
    if first:
        kids.reverse()
    cfg = {"nodes": kids, "pageHeader": {}, "pageDescription": {}}
    pg = fx.node("PageNode", cfg)
    listed = list_outcomes("PageNode", {"nodes": kids})
    log(f"--- variant {tag}: page listOutcomes = {listed}")
    nodes = {pg: ("PageNode", "page", {})}
    for oc in listed:
        d = fx.scripted(f"{tag}-dump{oc}", dump_source(KEYS, f"{tag}-{oc}"), ["true"])
        # a dump never completes, so its outcome is never taken
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": FAILURE})
        nodes[pg][2][oc] = d
    return fx.tree(tag, pg, nodes), prefix


def drive(tree, label, prefix):
    report(f"{label} baseline", prefix)
    r1 = authenticate(tree)
    show(f"{label} step1 (empty POST)", r1)
    report(f"{label} after step1", prefix)
    body = r1[1]
    if r1[0] != 200 or not isinstance(body, dict) or "callbacks" not in body:
        return
    vals = {}
    for i, c in enumerate(body["callbacks"]):
        if c["type"] == "NameCallback":
            prompt = {o["name"]: o.get("value") for o in c.get("output", [])}.get("prompt")
            vals[i] = "zzemu-script-answer" if prompt == "zzemu script prompt" else "zzemu-page-user"
    r2 = authenticate(tree, fill(body, vals))
    show(f"{label} step2 (page submitted)", r2)
    report(f"{label} after step2", prefix)


fx = Fixture("pgdisc")
try:
    for tag, kind, first in [("nocb-last", "nocb", False), ("nocb-first", "nocb", True),
                             ("cb-last", "cb", False), ("throw-last", "throw", False),
                             ("goto-last", "goto", False)]:
        tree, prefix = build(fx, tag, kind, first)
        drive(tree, f"variant {tag}", prefix)
finally:
    log("--- cleanup")
    run_prefix = f"zzemu-disc-{fx.run}-"
    try:
        for r in count(run_prefix):
            fx.track("user", f"/openidm/managed/{REALM}_user/{r['_id']}")
    except Exception as e:  # still delete the AM objects
        log("    record query failed:", e)
    for line in fx.cleanup():
        log("   ", line)
    left = count("zzemu-disc-")
    log(f"    zzemu-disc-* records remaining: {len(left)}")
PY
