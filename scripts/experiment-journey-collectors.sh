#!/usr/bin/env bash
# experiment-journey-collectors.sh
#
# Question this answers:
#   What callbacks do Attribute Collector, Validated Username and Validated
#   Password emit (types and outputs), what does a failed required/policy check
#   look like, and which state keys do they write (shared or transient)?
#
# Method:
#   "ac": a lone AttributeCollectorNode (givenName, sn, mail; no validation,
#   not required). "page": a Page of ValidatedUsername + AttributeCollector
#   (required, validated) + ValidatedPassword, submitted first with mail empty
#   (the control for the required/policy path) and then complete. A first
#   round leaves the username and password empty. Each is
#   followed by a dump node that reports state on its first pass and again
#   after its own callback round trip (transient keys vanish). Password values
#   are fake and the dump masks any password value it prints.
#
# Every object is named zzemu-* and deleted on exit. No user is created.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, list_outcomes)

KEYS = ["username", "password", "objectAttributes", "givenName", "mail"]
ATTRS = ["givenName", "sn", "mail"]
fx = Fixture("coll")


def answer(body, vals):
    body = json.loads(json.dumps(body))
    for c in body.get("callbacks", []):
        out = {o["name"]: o.get("value") for o in c.get("output", [])}
        key = out.get("name") or c["type"]
        if key in vals and c.get("input"):
            c["input"][0]["value"] = vals[key]
    return body


def follow(tree, label, rounds):
    r = authenticate(tree)
    show(f"{label} step1", r)
    for i, vals in enumerate(rounds, start=2):
        if r[0] != 200 or "callbacks" not in r[1]:
            return
        r = authenticate(tree, answer(r[1], vals))
        show(f"{label} step{i}", r)
    if r[0] == 200 and "callbacks" in r[1]:
        show(f"{label} dump resumed", authenticate(tree, r[1]))


try:
    ac_cfg = {"attributesToCollect": ATTRS, "identityAttribute": "userName",
              "validateInputs": False, "required": False}
    log("--- listOutcomes AttributeCollectorNode:", list_outcomes("AttributeCollectorNode", ac_cfg))
    ac = fx.node("AttributeCollectorNode", ac_cfg)
    d = fx.scripted("ac-dump", dump_source(KEYS, "ac"), ["true"])
    tree = fx.tree("ac", ac, {ac: ("AttributeCollectorNode", "attrs", {"outcome": d}),
                              d: ("ScriptedDecisionNode", "dump", {"true": SUCCESS})})
    full = {"givenName": "Zz", "sn": "Emu", "mail": "zzemu-coll@example.com"}
    follow(tree, "ac", [full])

    vu = fx.node("ValidatedUsernameNode", {"usernameAttribute": "userName", "validateInput": True,
                                           "autocompleteValues": []})
    ac2 = fx.node("AttributeCollectorNode", dict(ac_cfg, validateInputs=True, required=True))
    vp = fx.node("ValidatedPasswordNode", {"validateInput": False, "passwordAttribute": "password"})
    kids = [{"_id": vu, "nodeType": "ValidatedUsernameNode", "displayName": "vu"},
            {"_id": ac2, "nodeType": "AttributeCollectorNode", "displayName": "ac"},
            {"_id": vp, "nodeType": "ValidatedPasswordNode", "displayName": "vp"}]
    pg = fx.node("PageNode", {"nodes": kids, "pageHeader": {}, "pageDescription": {}})
    d2 = fx.scripted("page-dump", dump_source(KEYS, "page"), ["true"])
    tree2 = fx.tree("page", pg, {pg: ("PageNode", "page", {"outcome": d2}),
                                 d2: ("ScriptedDecisionNode", "dump", {"true": SUCCESS})})
    uname = fx.name("u")
    # Callbacks without a `name` output are answered by type.
    base = {"ValidatedCreateUsernameCallback": uname, "PasswordCallback": "Zz-emu-Passw0rd!x",
            "ValidatedCreatePasswordCallback": "Zz-emu-Passw0rd!x"}
    follow(tree2, "page", [{"givenName": "Zz", "sn": "Emu", "mail": "zzemu-coll@example.com"},
                           dict(base, givenName="Zz", sn="Emu", mail=""),
                           dict(base, givenName="Zz", sn="Emu", mail="zzemu-coll@example.com")])
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
