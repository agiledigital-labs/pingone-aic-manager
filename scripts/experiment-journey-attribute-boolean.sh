#!/usr/bin/env bash
# experiment-journey-attribute-boolean.sh
#
# Question: what does AttributeCollectorNode send and store for a BOOLEAN schema property (the
# sandbox realm has `preferences/remember`, title "Trust this device")?
#   first    the callbacks: type, fields, prompt, required, policies, default value
#   true     answered true            -> stored type and value
#   false    answered false           -> stored type and value
#   string   answered the string "true" -> refused? coerced?
#   req      required true, answered false -> does the page stay? (false is a value, not empty)
#   validate validateInputs true over the boolean -> policies sent, any failure possible
#   mixed    a string and a boolean attribute together, answered, to see the callback order
#
# Objects are kdemo-g3-*, deleted on exit, deletion confirmed by a GET. No user is created.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g3}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, dump_source, log, cbs, safe, SUCCESS)

KEYS = ["objectAttributes"]
fx = Fixture("attrbool")
BOOL = "preferences/remember"


def view(label, resp):
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
        log("   ", c["type"], json.dumps(o)[:700], "inputs", json.dumps(c["input"]))


def answer(body, values):
    body = json.loads(json.dumps(body))
    for c in body.get("callbacks", []):
        name = {o["name"]: o.get("value") for o in c.get("output", [])}.get("name")
        if name in values:
            c["input"][0]["value"] = values[name]
    return body


def run(tag, attrs, validate, required, answers_list):
    cfg = {"attributesToCollect": attrs, "identityAttribute": "userName",
           "validateInputs": validate, "required": required}
    ac = fx.node("AttributeCollectorNode", cfg)
    d = fx.scripted(f"{tag}-dump", dump_source(KEYS, tag), ["true"])
    tree = fx.tree(tag, ac, {ac: ("AttributeCollectorNode", "attrs", {"outcome": d}),
                             d: ("ScriptedDecisionNode", "dump", {"true": SUCCESS})})
    log(f"=== {tag}: attrs={attrs} validateInputs={validate} required={required}")
    r = authenticate(tree)
    view(f"{tag} first", r)
    for vals in answers_list:
        if r[0] != 200 or "callbacks" not in r[1]:
            return
        r = authenticate(tree, answer(r[1], vals))
        view(f"{tag} answered {json.dumps(vals)}", r)


try:
    run("btrue", [BOOL], False, False, [{BOOL: True}])
    run("bfalse", [BOOL], False, False, [{BOOL: False}])
    run("bstring", [BOOL], False, False, [{BOOL: "true"}])
    run("breq", [BOOL], False, True, [{BOOL: False}])
    run("bval", [BOOL], True, False, [{BOOL: True}])
    run("mixed", [BOOL, "givenName"], False, False, [{BOOL: True, "givenName": "Zz"}])
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
