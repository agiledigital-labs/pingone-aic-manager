#!/usr/bin/env bash
# experiment-journey-patch-ignored.sh
#
# Question: what does Patch Object's `ignoredFields` do? Each variant gets a fresh user
# (givenName G0, sn S0, mail M0), seeds objectAttributes {userName, givenName G1, sn S1}, and runs
# the node with the ignoredFields under test; the IDM record is read back after the run, and the
# outcome (PATCHED / FAILURE) comes from the dump node on that branch.
#   none           ignoredFields []                         (control: both fields change)
#   sn             ["sn"]
#   nonexistent    ["zzz"]
#   both           ["sn","givenName"]
#   identity       ["userName"]  (the identity attribute itself)
#   pointer        ["/sn"]
#   all            ["userName","sn","givenName"]  (every seeded field)
#   pointerboth    ["/sn","givenName"]  (a pointer beside a bare name)
#   case           ["SN"]
#   split-transient ["sn"] with sn in transient objectAttributes, userName and givenName shared
#   absent         no ignoredFields key (control)
#
# Objects are kdemo-g2-*, deleted on exit.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g2}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, call, list_outcomes, REALM)

KEYS = ["username", "_id", "objectAttributes"]
USERS = f"/openidm/managed/{REALM}_user"
fx = Fixture("patchign")


def seed_src(shared, transient):
    lines = []
    for k, v in shared.items():
        lines.append(f"nodeState.putShared({json.dumps(k)}, {json.dumps(v)});")
    for k, v in transient.items():
        lines.append(f"nodeState.putTransient({json.dumps(k)}, {json.dumps(v)});")
    lines.append('action.goTo("true");')
    return "\n".join(lines)


def run(tag, ignored, split=False):
    uname = fx.name(tag)
    uid = fx.idm_user({"userName": uname, "givenName": "G0", "sn": "S0", "mail": uname + "@example.com"})
    config = {"identityResource": f"managed/{REALM}_user", "patchAsObject": False,
              "identityAttribute": "userName"}
    if ignored is not None:
        config["ignoredFields"] = ignored
    shared = {"objectAttributes": {"userName": uname, "givenName": "G1"} if split
              else {"userName": uname, "givenName": "G1", "sn": "S1"}}
    transient = {"objectAttributes": {"sn": "S1"}} if split else {}
    node_type = "PatchObjectNode"
    outcomes = list_outcomes(node_type, config)
    seed = fx.scripted(f"{tag}-seed", seed_src(shared, transient), ["true"])
    nid = fx.node(node_type, config)
    nodes = {seed: ("ScriptedDecisionNode", "seed", {"true": nid}), nid: (node_type, node_type, {})}
    for oc in outcomes:
        d = fx.scripted(f"{tag}-d{oc}", dump_source(KEYS, oc), ["true"])
        nodes[nid][2][oc] = d
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
    tree = fx.tree(tag, seed, nodes)
    log(f"=== {tag}: ignoredFields={json.dumps(ignored)} split={split}")
    st, body, _ = authenticate(tree)
    show(f"{tag}", (st, body, None))
    st, b, _ = call("GET", f"{USERS}/{uid}?_fields=givenName,sn,mail,userName", api="resource=1.0")
    log(f"--- IDM after {tag}: givenName={b.get('givenName')} sn={b.get('sn')}"
        f" mailkept={str(b.get('mail', '')).endswith('@example.com')}"
        f" userNameKept={b.get('userName') == uname}")


try:
    run("none", [])
    run("absent", None)
    run("sn", ["sn"])
    run("nonexistent", ["zzz"])
    run("both", ["sn", "givenName"])
    run("identity", ["userName"])
    run("all", ["userName", "sn", "givenName"])
    run("pointerboth", ["/sn", "givenName"])
    run("pointer", ["/sn"])
    run("case", ["SN"])
    run("splittr", ["sn"], split=True)
    run("splitnone", [], split=True)
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
