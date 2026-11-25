#!/usr/bin/env bash
# experiment-journey-page-choice-otp.sh
#
# Question: what do OneTimePasswordCollectorDecisionNode and ChoiceCollectorNode send and decide
# when they are a Page's child (alone, after a UsernameCollectorNode), against the same node
# standing alone? Callbacks and their input names, the page's outcome for a right and a wrong
# one-time password, each choice index, an out-of-range index, and an empty one-time password.
#
# Method: a seed script writes shared oneTimePassword "123456" and a fresh oneTimePasswordTimestamp;
# the node (or page) follows; every outcome leads to a dump node. kdemo-g4-* objects, deleted on
# exit, deletion confirmed by a GET.
set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g4}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, fill, dump_source, log, SUCCESS, list_outcomes, call, NODES)

SEED = r"""
nodeState.putShared("oneTimePassword", "123456");
nodeState.putShared("oneTimePasswordTimestamp", Math.floor(new Date().getTime() / 1000));
action.goTo("true");
"""
KEYS = ["username", "oneTimePassword", "pageNodeCallbacks"]
fx = Fixture("po")
OTP = {"passwordExpiryTime": 5}
CHOICE = {"prompt": "Pick", "choices": ["alpha", "beta", "gamma"], "defaultChoice": "beta"}


def child(kind, cfg):
    return {"_id": fx.node(kind, cfg), "nodeType": kind, "displayName": f"kdemo {kind}"}


def build(tag, last_kind, cfg, with_username, standalone=False):
    outcomes = list_outcomes(last_kind, cfg)
    seed = fx.scripted(f"{tag}-seed", SEED, ["true"])
    if standalone:
        target = fx.node(last_kind, cfg)
        ttype = last_kind
    else:
        kids = ([child("UsernameCollectorNode", {})] if with_username else []) + [child(last_kind, cfg)]
        target = fx.node("PageNode", {"nodes": kids, "pageHeader": {}, "pageDescription": {}})
        ttype = "PageNode"
    nodes = {seed: ("ScriptedDecisionNode", "seed", {"true": target}), target: (ttype, "target", {})}
    for oc in outcomes:
        d = fx.scripted(f"{tag}-d{oc}", dump_source(KEYS, oc.upper()), ["true"])
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
        nodes[target][2][oc] = d
    log(f"--- {tag}: outcomes {outcomes}")
    return fx.tree(tag, seed, nodes)


def answers(body, vals):
    return fill(body, vals)


def drive(tag, tree, vals):
    r1 = authenticate(tree)
    show(f"{tag} step1", r1)
    if r1[0] != 200 or "callbacks" not in r1[1]:
        return
    log("    callback inputs:", [(c["type"], [i["name"] for i in c["input"]], [i.get("value") for i in c["input"]]) for c in r1[1]["callbacks"]])
    r2 = authenticate(tree, answers(r1[1], vals))
    show(f"{tag} step2 answered {vals}", r2)
    return r2


try:
    for kind, cfg in (("OneTimePasswordCollectorDecisionNode", OTP), ("ChoiceCollectorNode", CHOICE)):
        log(f"=== {kind}")
        for tag, with_user, standalone in (("alone", False, True), ("page", False, False), ("pageU", True, False)):
            short = kind[:3].lower()
            t = build(f"{short}-{tag}", kind, cfg, with_user, standalone)
            idx = 1 if with_user else 0
            if kind.startswith("OneTime"):
                tries = [("right", "123456"), ("wrong", "000000"), ("empty", "")]
            else:
                tries = [("idx0", 0), ("idx2", 2), ("idx7", 7)]
            for label, v in tries:
                vals = {idx: v}
                if with_user:
                    vals[0] = "kdemo-g4-user"
                drive(f"{kind} {tag} {label}", t, vals)
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
