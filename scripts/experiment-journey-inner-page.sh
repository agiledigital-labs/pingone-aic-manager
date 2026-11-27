#!/usr/bin/env bash
# experiment-journey-inner-page.sh
#
# Question: what does a PageNode do inside an inner tree? Which callbacks and header/description/
# stage reach the client, do an outer page's properties leak into it, how do shared and transient
# state cross the boundary across the page's submission, does resuming continue inside the inner
# tree (and then route the outer one by the inner outcome), and what do a re-visited page and two
# levels of nesting do?
#
# Method: kdemo-g4-* trees (all deleted on exit, deletion confirmed by GET). A dump node reports
# state; the inner trees' own scripts record what they saw into shared state for the outer dump.
#   A  outer: script -> inner evaluator -> dump.  inner: Page(props, [Username, Password]) -> script
#   B  outer: Page(OUTER props,[Username]) -> inner evaluator.  inner: Page(NO props,[Password]) -> script
#   C  inner page visited twice: Page([Username]) -> script (username == "right" ? success : back to Page)
#   D  nested two deep: outer -> evaluator -> mid(evaluator) -> inner Page
#   E  empty submission re-sends the inner page; the inner outcome "false" routes the outer evaluator's false
set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g4}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, fill, dump_source, log, SUCCESS, FAILURE)

KEYS = ["username", "password", "pageNodeCallbacks", "zzOuterShared", "zzOuterTransient", "zzInShared",
        "zzInTransient", "zzInSawUser", "zzInSawPw", "zzInSawOuterS", "zzInSawOuterT", "zzMidSawUser"]
OUTER = r"""
nodeState.putShared("zzOuterShared", "os");
nodeState.putTransient("zzOuterTransient", "ot");
action.goTo("true");
"""
INNER = r"""
nodeState.putShared("zzInSawUser", String(nodeState.get("username")));
var pw = nodeState.get("password");
nodeState.putShared("zzInSawPw", pw === null ? "null" : "len=" + String(pw).length);
nodeState.putShared("zzInSawOuterS", String(nodeState.get("zzOuterShared")));
nodeState.putShared("zzInSawOuterT", String(nodeState.get("zzOuterTransient")));
nodeState.putShared("zzInShared", "is");
nodeState.putTransient("zzInTransient", "it");
action.goTo("true");
"""
CHECK = r"""
nodeState.putShared("zzInSawUser", String(nodeState.get("username")));
action.goTo(String(nodeState.get("username")) === "right" ? "true" : "false");
"""
MID = r"""
nodeState.putShared("zzMidSawUser", String(nodeState.get("username")));
action.goTo("true");
"""
fx = Fixture("ip")


def child(kind, cfg=None):
    nid = fx.node(kind, cfg or {})
    return {"_id": nid, "nodeType": kind, "displayName": f"kdemo {kind}"}


def page(children, header="", desc="", stage=""):
    cfg = {"nodes": children, "pageHeader": {"en": header} if header else {},
           "pageDescription": {"en": desc} if desc else {}}
    if stage:
        cfg["stage"] = stage
    return fx.node("PageNode", cfg)


def dumps(tag):
    t = fx.scripted(f"{tag}-dT", dump_source(KEYS, "T"), ["true"])
    f = fx.scripted(f"{tag}-dF", dump_source(KEYS, "F"), ["true"])
    return t, f


def outer_over(tag, inner_name, pre=None):
    """outer tree: [pre] -> evaluator(inner) -> dump T / dump F."""
    it = fx.node("InnerTreeEvaluatorNode", {"tree": inner_name, "displayErrorOutcome": False})
    dt, df = dumps(tag)
    nodes = {it: ("InnerTreeEvaluatorNode", "inner", {"true": dt, "false": df}),
             dt: ("ScriptedDecisionNode", "dump T", {"true": SUCCESS}),
             df: ("ScriptedDecisionNode", "dump F", {"true": SUCCESS})}
    entry = it
    if pre is not None:
        nodes = {pre[0]: pre[1], **nodes}
        entry = pre[2]
    return fx.tree(f"{tag}-out", entry, nodes)


def step(tag, tree, resp, values, label):
    body = resp[1]
    r = authenticate(tree, fill(body, values) if values is not None else body)
    show(f"{tag} {label}", r)
    return r


try:
    # A: inner page with properties, no outer page.
    pg = page([child("UsernameCollectorNode"), child("PasswordCollectorNode")], "IN-H", "IN-D", "IN-S")
    sc = fx.scripted("a-in", INNER, ["true"])
    inner = fx.tree("a-in", pg, {pg: ("PageNode", "inner page", {"outcome": sc}),
                                 sc: ("ScriptedDecisionNode", "inner script", {"true": SUCCESS})}, inner_only=True)
    so = fx.scripted("a-o", OUTER, ["true"])
    log("--- A: outer script -> evaluator -> inner Page(props)")
    it = fx.node("InnerTreeEvaluatorNode", {"tree": inner, "displayErrorOutcome": False})
    dt, df = dumps("a")
    outer = fx.tree("a-out", so, {
        so: ("ScriptedDecisionNode", "outer", {"true": it}),
        it: ("InnerTreeEvaluatorNode", "inner", {"true": dt, "false": df}),
        dt: ("ScriptedDecisionNode", "dump T", {"true": SUCCESS}),
        df: ("ScriptedDecisionNode", "dump F", {"true": SUCCESS})})
    r1 = authenticate(outer)
    show("A step1", r1)
    if r1[0] == 200 and "callbacks" in r1[1]:
        log("    top-level keys:", sorted(k for k in r1[1] if k not in ("callbacks", "authId")),
            {k: r1[1][k] for k in ("header", "description", "stage") if k in r1[1]})
        r2 = step("A", outer, r1, {0: "kdemo-g4-user", 1: "kdemo-pw-1234"}, "step2 (answered; dump pass 1)")
        if r2[0] == 200 and "callbacks" in r2[1]:
            step("A", outer, r2, None, "step3 (dump resumed, pass 2)")

    # B: outer page with props, then inner page WITHOUT props.
    opg = page([child("UsernameCollectorNode")], "OUT-H", "OUT-D", "OUT-S")
    pgb = page([child("PasswordCollectorNode")])
    scb = fx.scripted("b-in", INNER, ["true"])
    innerb = fx.tree("b-in", pgb, {pgb: ("PageNode", "inner page", {"outcome": scb}),
                                   scb: ("ScriptedDecisionNode", "inner script", {"true": SUCCESS})}, inner_only=True)
    itb = fx.node("InnerTreeEvaluatorNode", {"tree": innerb, "displayErrorOutcome": False})
    dt, df = dumps("b")
    outerb = fx.tree("b-out", opg, {
        opg: ("PageNode", "outer page", {"outcome": itb}),
        itb: ("InnerTreeEvaluatorNode", "inner", {"true": dt, "false": df}),
        dt: ("ScriptedDecisionNode", "dump T", {"true": SUCCESS}),
        df: ("ScriptedDecisionNode", "dump F", {"true": SUCCESS})})
    log("--- B: outer Page(OUT props) -> evaluator -> inner Page(no props)")
    r1 = authenticate(outerb)
    show("B step1 (outer page)", r1)
    r2 = step("B", outerb, r1, {0: "kdemo-g4-outer"}, "step2 (outer answered; the inner page)")
    log("    top-level keys:", sorted(k for k in r2[1] if k not in ("callbacks", "authId")) if isinstance(r2[1], dict) else r2[1])
    r3 = step("B", outerb, r2, {0: "kdemo-pw-1234"}, "step3 (inner answered; dump pass 1)")
    if r3[0] == 200 and "callbacks" in r3[1]:
        step("B", outerb, r3, None, "step4 (dump resumed)")

    # C: inner page visited twice.
    pgc = page([child("UsernameCollectorNode")], "C-H", "C-D")
    scc = fx.scripted("c-in", CHECK, ["true", "false"])
    innerc = fx.tree("c-in", pgc, {pgc: ("PageNode", "inner page", {"outcome": scc}),
                                   scc: ("ScriptedDecisionNode", "check", {"true": SUCCESS, "false": pgc})}, inner_only=True)
    outerc = outer_over("c", innerc)
    log("--- C: inner page revisited: wrong, wrong, right")
    r = authenticate(outerc)
    show("C step1", r)
    for n, val in enumerate(["wrong1", "wrong2", "right"], start=2):
        r = step("C", outerc, r, {0: val}, f"step{n} (answered {val})")
        if r[0] != 200 or "callbacks" not in r[1]:
            break
    if r[0] == 200 and isinstance(r[1], dict) and "callbacks" in r[1] and any(
            c["type"] == "HiddenValueCallback" for c in r[1]["callbacks"]):
        step("C", outerc, r, None, "step5 (dump resumed)")

    # D: nested two deep.
    pgd = page([child("UsernameCollectorNode")], "D-H")
    scd = fx.scripted("d-in", INNER, ["true"])
    inner_d = fx.tree("d-in", pgd, {pgd: ("PageNode", "deep page", {"outcome": scd}),
                                    scd: ("ScriptedDecisionNode", "deep script", {"true": SUCCESS})}, inner_only=True)
    itd = fx.node("InnerTreeEvaluatorNode", {"tree": inner_d, "displayErrorOutcome": False})
    scm = fx.scripted("d-mid", MID, ["true"])
    mid = fx.tree("d-mid", itd, {itd: ("InnerTreeEvaluatorNode", "to deep", {"true": scm, "false": FAILURE}),
                                 scm: ("ScriptedDecisionNode", "mid script", {"true": SUCCESS})}, inner_only=True)
    outerd = outer_over("d", mid)
    log("--- D: outer -> mid (evaluator) -> deep Page")
    r = authenticate(outerd)
    show("D step1", r)
    r = step("D", outerd, r, {0: "kdemo-g4-deep"}, "step2 (answered; dump pass 1)")
    if r[0] == 200 and isinstance(r[1], dict) and "callbacks" in r[1]:
        step("D", outerd, r, None, "step3 (dump resumed)")

    # E: empty submission re-sends; inner outcome false routes outer false.
    pge = page([child("UsernameCollectorNode"), child("PasswordCollectorNode")], "E-H", "E-D")
    ske = fx.scripted("e-in", CHECK, ["true", "false"])
    innere = fx.tree("e-in", pge, {pge: ("PageNode", "inner page", {"outcome": ske}),
                                   ske: ("ScriptedDecisionNode", "check", {"true": SUCCESS, "false": FAILURE})}, inner_only=True)
    outere = outer_over("e", innere)
    log("--- E: empty answers re-send; then a wrong name leaves the inner tree by failure (outer false)")
    r = authenticate(outere)
    show("E step1", r)
    r2 = step("E", outere, r, {0: "", 1: ""}, "step2 (empty answers)")
    if r2[0] == 200 and isinstance(r2[1], dict) and "callbacks" in r2[1]:
        r3 = step("E", outere, r2, {0: "nope", 1: "kdemo-pw-1234"}, "step3 (wrong name -> inner false -> outer F dump)")
        if r3[0] == 200 and isinstance(r3[1], dict) and "callbacks" in r3[1]:
            step("E", outere, r3, None, "step4 (dump resumed)")
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
