#!/usr/bin/env bash
# experiment-journey-flow.sh
#
# Question this answers:
#   (1) Does transient state survive a node boundary with no callback between
#   (Scripted -> Scripted)? (2) What do failure responses look like: the
#   Failure node reached plainly, with withErrorMessage / withLockoutMessage,
#   a script that throws, an unknown tree, a replayed or corrupted authId?
#   (3) When a scripted pass queues callbacks AND calls goTo, which wins, and
#   what does the resumed pass see?
#
# Method:
#   (1) S1 writes zzT (transient) and zzS (shared), S2 records what it reads,
#   then a dump that reports on pass 1 and again after its own round trip (the
#   round trip is the control: zzT must vanish there). (2) One tree per failure
#   variant; "plain" is the control the message variants are compared against.
#   (3) "cb-and-goto" queues a TextOutputCallback and calls goTo("true");
#   "goto-only" is identical minus the callback (the control). (4) Deleting a
#   tree: GET a node inside it and one outside any tree, before and after.
#
# Every object is named zzemu-* and deleted on exit. A successful journey's
# tokenId is reported only as its length.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
import urllib.parse
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, FAILURE, call, NODES, TREES)

fx = Fixture("flow")


def one(tag, src, outcomes=("true", "false")):
    """Tree: script -> true:SUCCESS, false:FAILURE."""
    s = fx.scripted(tag, src, list(outcomes))
    conns = {"true": SUCCESS, "false": FAILURE}
    return fx.tree(tag, s, {s: ("ScriptedDecisionNode", tag, {o: conns[o] for o in outcomes})})


try:
    # (1) transient across a callback-free boundary
    s1 = fx.scripted("t-s1", 'nodeState.putTransient("zzT", "t1");\n'
                     'nodeState.putShared("zzS", "s1");\naction.goTo("true");', ["true"])
    s2 = fx.scripted("t-s2", 'nodeState.putShared("zzS2SawT", String(nodeState.get("zzT")));\n'
                     'nodeState.putShared("zzS2SawS", String(nodeState.get("zzS")));\naction.goTo("true");',
                     ["true"])
    d = fx.scripted("t-dump", dump_source(["zzT", "zzS", "zzS2SawT", "zzS2SawS"], "t"), ["true"])
    tree = fx.tree("transient", s1, {s1: ("ScriptedDecisionNode", "s1", {"true": s2}),
                                      s2: ("ScriptedDecisionNode", "s2", {"true": d}),
                                      d: ("ScriptedDecisionNode", "dump", {"true": SUCCESS})})
    r = authenticate(tree)
    show("transient pass1", r)
    if r[0] == 200 and "callbacks" in r[1]:
        show("transient pass2 (dump resumed)", authenticate(tree, r[1]))

    # (2) failure shapes
    for tag, src in [
            ("f-plain", 'action.goTo("false");'),
            ("f-errmsg-after", 'action.goTo("false").withErrorMessage("zzemu error after");'),
            ("f-errmsg-before", 'action.withErrorMessage("zzemu error before").goTo("false");'),
            ("f-lockout", 'action.goTo("false").withLockoutMessage("zzemu lockout");'),
            ("f-errmsg-on-success", 'action.goTo("true").withErrorMessage("zzemu error on success");'),
            ("f-throw", 'throw new Error("zzemu boom");'),
            ("f-unknown-outcome", 'action.goTo("nope");'),
            ("s-plain", 'nodeState.putShared("username", "zzemu-nobody"); action.goTo("true");'),
            # controls for f-errmsg-on-success: success with no username, and
            # success with a username plus an error message
            ("s-nouser", 'action.goTo("true");'),
            ("s-errmsg-with-user", 'nodeState.putShared("username", "zzemu-nobody");\n'
                                   'action.goTo("true").withErrorMessage("zzemu error on success");'),
            # declared on the node but not connected in the tree
            ("f-unwired", 'action.goTo("other");')]:
        tree = one(tag, src) if tag != "f-unwired" else None
        if tree is None:
            s = fx.scripted(tag, src, ["true", "false", "other"])
            tree = fx.tree(tag, s, {s: ("ScriptedDecisionNode", tag, {"true": SUCCESS, "false": FAILURE})})
        st, body, hdr = authenticate(tree)
        log(f"--- {tag}: HTTP {st}")
        log("   ", json.dumps({k: (v if k != "tokenId" else f"<len={len(v)}>") for k, v in body.items()})
            if isinstance(body, dict) else body)
    st, body, _ = authenticate(fx.name("does-not-exist"))
    log(f"--- unknown tree: HTTP {st}\n    {json.dumps(body)}")

    # replayed / corrupted authId
    d2 = fx.scripted("r-dump", dump_source(["zzS"], "r"), ["true"])
    tree = fx.tree("replay", d2, {d2: ("ScriptedDecisionNode", "dump", {"true": SUCCESS})})
    r1 = authenticate(tree)
    r2 = authenticate(tree, r1[1])
    show("replay: first resume (fresh authId)", r2)
    show("replay: resubmit the first response again (stale authId)", authenticate(tree, r1[1]))
    bad = json.loads(json.dumps(r2[1]))
    bad["authId"] = bad["authId"][:-8] + "AAAAAAAA"
    show("replay: corrupted authId", authenticate(tree, bad))
    nob = json.loads(json.dumps(r2[1]))
    del nob["authId"]
    show("replay: callbacks with no authId", authenticate(tree, nob))

    # (4) does deleting a tree delete its nodes? A node outside any tree is
    # the control.
    inside = fx.scripted("del-inside", 'action.goTo("true");', ["true"])
    outside = fx.scripted("del-outside", 'action.goTo("true");', ["true"])
    tname = fx.tree("delete", inside, {inside: ("ScriptedDecisionNode", "inside", {"true": SUCCESS})})
    path = f"{TREES}/{urllib.parse.quote(tname)}"
    for label, nid in [("inside", inside), ("outside", outside)]:
        log(f"--- before tree DELETE, GET {label} node: {call('GET', f'{NODES}/ScriptedDecisionNode/{nid}')[0]}")
    log(f"--- tree DELETE: {call('DELETE', path)[0]}")
    for label, nid in [("inside", inside), ("outside", outside)]:
        log(f"--- after tree DELETE, GET {label} node: {call('GET', f'{NODES}/ScriptedDecisionNode/{nid}')[0]}")

    # (3) callbacks queued and goTo called on the same pass
    for tag, emit in [("cb-and-goto", True), ("goto-only", False)]:
        src = ('nodeState.putShared("zzPasses", String(Number(nodeState.get("zzPasses") || 0) + 1));\n'
               'nodeState.putShared("zzSawCallbacks", String(!callbacks.isEmpty()));\n')
        if emit:
            src += 'if (callbacks.isEmpty()) { callbacksBuilder.textOutputCallback(0, "zzemu cb"); }\n'
        src += 'action.goTo("true");'
        s = fx.scripted(tag, src, ["true"])
        dd = fx.scripted(tag + "-dump", dump_source(["zzPasses", "zzSawCallbacks"], tag), ["true"])
        tree = fx.tree(tag, s, {s: ("ScriptedDecisionNode", tag, {"true": dd}),
                                dd: ("ScriptedDecisionNode", "dump", {"true": SUCCESS})})
        r1 = authenticate(tree)
        show(f"{tag} step1", r1)
        if r1[0] == 200 and "callbacks" in r1[1] and not any(
                c["type"] == "HiddenValueCallback" for c in r1[1]["callbacks"]):
            show(f"{tag} step2 (callbacks submitted)", authenticate(tree, r1[1]))
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
