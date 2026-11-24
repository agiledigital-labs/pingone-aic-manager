#!/usr/bin/env bash
# experiment-journey-attribute-validate.sh
#
# Question: what does AttributeCollectorNode with `validateInputs: true` emit, and what does it do
# with a value that fails the attribute's IDM policy?
#   step 0   the first callbacks: the `policies` output of each attribute, in full
#   valid    a value that passes
#   badmail  `mail` that is not an address        -> page re-sent? `failedPolicies`? state?
#   empty    `mail` empty with required true      -> as above
#   emptyopt `mail` empty with required false     -> as above (non-required)
#   long     `givenName` of 600 characters        -> a length policy?
#   only     `validateOnly` true on a bad `mail`  -> sent again, node not left?
#   control  the same bad `mail` with validateInputs false: accepted unvalidated
#
# Objects are kdemo-g2-*, deleted on exit. No user is created.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g2}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, dump_source, log, cbs, safe,
                                    SUCCESS, list_outcomes)

KEYS = ["objectAttributes", "givenName", "mail"]
ATTRS = ["givenName", "sn", "mail"]
fx = Fixture("attrval")


def fill(body, vals, only=None):
    body = json.loads(json.dumps(body))
    for c in body.get("callbacks", []):
        out = {o["name"]: o.get("value") for o in c.get("output", [])}
        name = out.get("name")
        if name in vals:
            c["input"][0]["value"] = vals[name]
        if only and name in only:
            c["input"][1]["value"] = True
    return body


def view(label, resp, full=False):
    st, body, _ = resp
    log(f"--- {label}: HTTP {st}")
    if not isinstance(body, dict) or "callbacks" not in body:
        log("   ", json.dumps(safe(body))[:300])
        return
    for c in cbs(body):
        o = c["output"]
        if c["type"] == "HiddenValueCallback":
            log("    hidden", o.get("id"), "=", str(o.get("value"))[:300])
            continue
        shown = {k: o.get(k) for k in ("name", "required", "validateOnly", "value", "failedPolicies")}
        log("   ", c["type"], json.dumps(shown), "inputs", c["input"])
        if full:
            log("      policies:", json.dumps(o.get("policies")))
    log("    keys:", sorted(k for k in body if k not in ("callbacks",)))


def run(tag, validate, required, rounds):
    cfg = {"attributesToCollect": ATTRS, "identityAttribute": "userName",
           "validateInputs": validate, "required": required}
    ac = fx.node("AttributeCollectorNode", cfg)
    d = fx.scripted(f"{tag}-dump", dump_source(KEYS, tag), ["true"])
    tree = fx.tree(tag, ac, {ac: ("AttributeCollectorNode", "attrs", {"outcome": d}),
                             d: ("ScriptedDecisionNode", "dump", {"true": SUCCESS})})
    log(f"=== {tag}: validateInputs={validate} required={required}")
    r = authenticate(tree)
    view(f"{tag} step0", r, full=(tag == "valid"))
    for i, (vals, only) in enumerate(rounds, start=1):
        if r[0] != 200 or "callbacks" not in r[1]:
            return
        r = authenticate(tree, fill(r[1], vals, only))
        view(f"{tag} round{i} {json.dumps(vals)[:80]}{' validateOnly=' + str(only) if only else ''}", r)
    if r[0] == 200 and isinstance(r[1], dict) and "callbacks" in r[1]:
        pass


good = {"givenName": "Zz", "sn": "Emu", "mail": "kdemo-g2-attr@example.com"}
try:
    log("--- listOutcomes:", list_outcomes("AttributeCollectorNode", {
        "attributesToCollect": ATTRS, "identityAttribute": "userName", "validateInputs": True, "required": True}))
    run("valid", True, True, [(good, None)])
    run("badmail", True, True, [(dict(good, mail="not-an-email"), None), (good, None)])
    run("empty", True, True, [(dict(good, mail=""), None)])
    run("emptyopt", True, False, [(dict(good, mail=""), None)])
    run("long", True, True, [(dict(good, givenName="x" * 600), None)])
    run("only", True, True, [(dict(good, mail="not-an-email"), {"mail"}), (dict(good, mail="not-an-email"), None)])
    run("control", False, True, [(dict(good, mail="not-an-email"), None)])
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
