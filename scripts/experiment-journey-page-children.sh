#!/usr/bin/env bash
# experiment-journey-page-children.sh
#
# Question: what do ValidatedUsernameNode, MessageNode and GetAuthenticatorAppNode send and write
# when they are a Page's child (alone, after a UsernameCollectorNode, and before one), against
# the same node standing alone? GetAuthenticatorAppNode has no emulation at all, so it is also
# measured standing alone: callbacks, outcomes, answers, state written.
#
# Method: each case is a tree whose outcomes lead to dump nodes that report the outcome and the
# state keys; the page is driven with an empty POST, then answered. Objects are kdemo-g3-*,
# deleted on exit, deletion confirmed by a GET.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g3}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, fill, dump_source, log, SUCCESS,
                                    call, NODES, list_outcomes, cbs)

KEYS = ["username", "objectAttributes", "pageNodeCallbacks"]
fx = Fixture("pg")


def child(kind, cfg):
    nid = fx.node(kind, cfg)
    return {"_id": nid, "nodeType": kind, "displayName": f"kdemo {kind}"}


def tree_for(tag, page_children, outcomes):
    pg = fx.node("PageNode", {"nodes": page_children, "pageHeader": {}, "pageDescription": {}})
    nodes = {pg: ("PageNode", "page", {})}
    for oc in outcomes:
        d = fx.scripted(f"{tag}-d{oc}", dump_source(KEYS, oc.upper()), ["true"])
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
        nodes[pg][2][oc] = d
    return fx.tree(tag, pg, nodes)


def answers(body, confirm=0):
    vals = {}
    for i, c in enumerate(body.get("callbacks", [])):
        t = c["type"]
        if t == "NameCallback":
            vals[i] = "kdemo-g3-user"
        elif t == "PasswordCallback":
            vals[i] = "kdemo-pw"
        elif t == "ConfirmationCallback":
            vals[i] = confirm
        elif t == "ValidatedCreateUsernameCallback":
            vals[i] = "kdemo-g3-valid"
    return vals


def drive(tag, tree, confirm=0, followups=1):
    r1 = authenticate(tree)
    show(f"{tag} step1", r1)
    if r1[0] != 200 or "callbacks" not in r1[1]:
        return
    body = r1[1]
    log("    top-level keys of step1:", sorted(k for k in body if k not in ("callbacks", "authId")),
        {k: body[k] for k in ("header", "description", "stage") if k in body})
    r2 = authenticate(tree, fill(body, answers(body, confirm)))
    show(f"{tag} step2 (answered)", r2)
    nxt = r2[1]
    for n in range(followups):
        if r2[0] == 200 and isinstance(nxt, dict) and "callbacks" in nxt:
            r2 = authenticate(tree, nxt)
            show(f"{tag} step{3 + n} (dump resumed)", r2)
            nxt = r2[1]


def listed(kind, cfg):
    try:
        return list_outcomes(kind, cfg)
    except RuntimeError as e:
        return f"ERR {str(e)[:120]}"


try:
    cfgs = {
        "MessageNode": {"message": {"en": "kdemo message"}, "messageYes": {"en": "Go"}, "messageNo": {"en": "Stop"}},
        "ValidatedUsernameNode": {"usernameAttribute": "userName", "validateInput": False, "autocompleteValues": []},
        "GetAuthenticatorAppNode": {"continueLabel": {"en": "Continue"}, "message": {"en": "Get the app {{appleLink}} {{googleLink}}"},
                                    "googleLink": "https://play.google.com/store/apps/details?id=example", "appleLink": "https://apps.apple.com/app/example"},
    }
    for kind, cfg in cfgs.items():
        log(f"--- {kind} listOutcomes: {listed(kind, cfg)}")
    # Standalone.
    for kind, cfg in cfgs.items():
        oc = listed(kind, cfg)
        if isinstance(oc, str):
            continue
        n = fx.node(kind, cfg)
        nodes = {n: (kind, kind, {})}
        for o in oc:
            d = fx.scripted(f"alone-{kind[:6]}-d{o}", dump_source(KEYS, o.upper()), ["true"])
            nodes[d] = ("ScriptedDecisionNode", f"dump {o}", {"true": SUCCESS})
            nodes[n][2][o] = d
        t = fx.tree(f"alone-{kind[:6]}", n, nodes)
        drive(f"{kind} alone", t, confirm=0, followups=1)
    # As page children: last alone, last after a collector, and not last (config check).
    for kind, cfg in cfgs.items():
        oc = listed(kind, cfg)
        if isinstance(oc, str):
            continue
        t = tree_for(f"only-{kind[:6]}", [child(kind, cfg)], oc)
        drive(f"{kind} only child of a page", t)
        u = child("UsernameCollectorNode", {})
        t = tree_for(f"aft-{kind[:6]}", [u, child(kind, cfg)], oc)
        drive(f"{kind} after a username collector", t)
        kid = child(kind, cfg)
        u2 = child("UsernameCollectorNode", {})
        st, b, _ = call("PUT", f"{NODES}/PageNode/00000000-0000-4000-8000-0000000000{kind[0].lower() == 'm' and '21' or '22'}",
                        {"nodes": [kid, u2]})
        log(f"--- {kind} BEFORE a username collector: page PUT {st} {json.dumps(b)[:160]}")
        if st in (200, 201):
            fx.track("node", f"{NODES}/PageNode/00000000-0000-4000-8000-0000000000{kind[0].lower() == 'm' and '21' or '22'}")
    # Follow-ups: the whole GetAuthenticatorApp message text, the default configuration, the
    # callback index inside a page, which of two username writers wins, and out-of-range answers.
    def full(tag, tree, vals_fn=None):
        r1 = authenticate(tree)
        if r1[0] != 200 or "callbacks" not in r1[1]:
            log(f"--- {tag}: HTTP {r1[0]} {json.dumps(r1[1])[:200]}")
            return None
        for c in cbs(r1[1]):
            if c["type"] == "TextOutputCallback":
                log(f"--- {tag}: text {json.dumps(c['output'])}")
            else:
                log(f"--- {tag}: {c['type']} {json.dumps(c['output'])} input={c['input']}")
        return r1

    def alone_tree(tag, kind, cfg, oc):
        n = fx.node(kind, cfg)
        nodes = {n: (kind, kind, {})}
        for o in oc:
            d = fx.scripted(f"{tag}-d{o}", dump_source(KEYS, o.upper()), ["true"])
            nodes[d] = ("ScriptedDecisionNode", f"dump {o}", {"true": SUCCESS})
            nodes[n][2][o] = d
        return fx.tree(tag, n, nodes)

    full("GetAuthenticatorApp default config {}", alone_tree("gdef", "GetAuthenticatorAppNode", {}, ["outcome"]))
    full("GetAuthenticatorApp configured, alone", alone_tree("gcfg", "GetAuthenticatorAppNode", cfgs["GetAuthenticatorAppNode"], ["outcome"]))
    u = child("UsernameCollectorNode", {})
    full("GetAuthenticatorApp after a collector", tree_for("gpg", [u, child("GetAuthenticatorAppNode", cfgs["GetAuthenticatorAppNode"])], ["outcome"]))
    # Which writer wins: two username callbacks answered differently.
    t = tree_for("two", [child("UsernameCollectorNode", {}), child("ValidatedUsernameNode", cfgs["ValidatedUsernameNode"])], ["outcome"])
    r1 = authenticate(t)
    r2 = authenticate(t, fill(r1[1], {0: "kdemo-first", 1: "kdemo-second"}))
    show("two username writers answered first/second", r2)
    t = tree_for("two2", [child("ValidatedUsernameNode", cfgs["ValidatedUsernameNode"]), child("UsernameCollectorNode", {})], ["outcome"])
    r1 = authenticate(t)
    show("two username writers, validated first (step1)", r1)
    r2 = authenticate(t, fill(r1[1], {0: "kdemo-first", 1: "kdemo-second"}))
    show("two username writers, validated first, answered first/second", r2)
    # Empty answers.
    t = tree_for("emp", [child("ValidatedUsernameNode", cfgs["ValidatedUsernameNode"])], ["outcome"])
    r1 = authenticate(t)
    show("ValidatedUsername page child, empty answer", authenticate(t, fill(r1[1], {0: ""})))
    # Message answers: index 1 (No), and out of range.
    for ans in (1, 5):
        t = tree_for(f"msg{ans}", [child("MessageNode", cfgs["MessageNode"])], ["true", "false"])
        r1 = authenticate(t)
        show(f"MessageNode page child answered {ans}", authenticate(t, fill(r1[1], {1: ans})))
    for ans in (1, 7):
        t = tree_for(f"gans{ans}", [child("GetAuthenticatorAppNode", cfgs["GetAuthenticatorAppNode"])], ["outcome"])
        r1 = authenticate(t)
        show(f"GetAuthenticatorApp page child answered {ans}", authenticate(t, fill(r1[1], {1: ans})))
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
