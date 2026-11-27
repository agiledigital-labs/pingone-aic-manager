#!/usr/bin/env bash
# experiment-journey-identity-empty.sh
#
# Question: what does Identify Existing User do when state holds NO usable identity value?
#   none          nothing seeded at all
#   oa-empty      shared objectAttributes = {}
#   oa-other      shared objectAttributes = {sn} (no identityAttribute key, no username)
#   username-empty shared username = ""
#   username-null shared username = null
#   oa-null       shared objectAttributes = {mail: null}, no username
#   both-null     objectAttributes = {mail: null} and username = null
#   transient-null the same null in the transient bucket
# Controls: a seeded, existing mail (must be true), and a miss with a value (must be false), so
# an outcome that equals the miss is not mistaken for "no value".
#
# Method as experiment-journey-identity-nodes.sh: seed script -> node -> one dump per outcome.
# Objects are kdemo-g2-*, deleted on exit.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g2}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, list_outcomes, REALM)

KEYS = ["username", "_id", "objectAttributes", "userName", "mail", "zzSeeded"]
fx = Fixture("ieuempty")


def seed_src(shared, transient):
    lines = ['nodeState.putShared("zzSeeded", "yes");']
    for k, v in shared.items():
        lines.append(f"nodeState.putShared({json.dumps(k)}, {json.dumps(v)});")
    for k, v in transient.items():
        lines.append(f"nodeState.putTransient({json.dumps(k)}, {json.dumps(v)});")
    lines.append('action.goTo("true");')
    return "\n".join(lines)


def run(tag, config, shared, transient):
    node_type = "IdentifyExistingUserNode"
    outcomes = list_outcomes(node_type, config)
    seed = fx.scripted(f"{tag}-seed", seed_src(shared, transient), ["true"])
    nid = fx.node(node_type, config)
    nodes = {seed: ("ScriptedDecisionNode", "seed", {"true": nid}),
             nid: (node_type, node_type, {})}
    for oc in outcomes:
        d = fx.scripted(f"{tag}-d{oc}", dump_source(KEYS, oc), ["true"])
        nodes[nid][2][oc] = d
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
    tree = fx.tree(tag, seed, nodes)
    log(f"=== {tag}: shared={json.dumps(shared)} transient={json.dumps(transient)}")
    r1 = authenticate(tree)
    show(f"{tag} pass1", r1)


try:
    uname = fx.name("user")
    mail = f"{uname}@example.com"
    fx.idm_user({"userName": uname, "givenName": "Zz", "sn": "Emu", "mail": mail})
    ieu = {"identityAttribute": "mail", "identifier": "userName"}
    run("ctl-hit", ieu, {"objectAttributes": {"mail": mail}}, {})
    run("ctl-miss", ieu, {"objectAttributes": {"mail": "kdemo-g2-nobody@example.com"}}, {})
    run("none", ieu, {}, {})
    run("oa-empty", ieu, {"objectAttributes": {}}, {})
    run("oa-other", ieu, {"objectAttributes": {"sn": "x"}}, {})
    run("username-empty", ieu, {"username": ""}, {})
    run("username-null", ieu, {"username": None}, {})
    run("oa-null", ieu, {"objectAttributes": {"mail": None}}, {})
    run("both-null", ieu, {"objectAttributes": {"mail": None}, "username": None}, {})
    run("transient-null", ieu, {}, {"objectAttributes": {"mail": None}})
    run("uname-none", {"identityAttribute": "userName", "identifier": "userName"}, {}, {})
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
