#!/usr/bin/env bash
# experiment-journey-identify-key.sh
#
# Question: which shared (or transient) key does Identify Existing User read as its lookup value
# when objectAttributes has no identityAttribute: `username`, `userName`, or both; which wins when
# both are set to two different users?
#
# Method: two throwaway alpha_users A (userName UA) and B (userName UB); IdentifyExistingUserNode
# with identityAttribute and identifier "userName"; a seed script writes exactly the keys under
# test; each outcome leads to a dump of username, userName, _id and objectAttributes. kdemo-g4-*,
# all deleted on exit, deletion confirmed by a GET.
set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g4}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log, SUCCESS, list_outcomes)

KEYS = ["username", "userName", "_id", "objectAttributes"]
IEU = {"identityAttribute": "userName", "identifier": "userName"}
fx = Fixture("ik")
try:
    ua = fx.idm_user({"userName": "kdemo-g4-ua", "givenName": "Zz", "sn": "Emu", "mail": "kdemo-g4-ua@example.com"})
    ub = fx.idm_user({"userName": "kdemo-g4-ub", "givenName": "Zz", "sn": "Emu", "mail": "kdemo-g4-ub@example.com"})
    log(f"ids: UA={ua} UB={ub}")
    variants = {
        "lower-only": ("shared", {"username": "kdemo-g4-ua"}),
        "camel-only": ("shared", {"userName": "kdemo-g4-ua"}),
        "both-lower-A": ("shared", {"username": "kdemo-g4-ua", "userName": "kdemo-g4-ub"}),
        "both-camel-A": ("shared", {"username": "kdemo-g4-ub", "userName": "kdemo-g4-ua"}),
        "transient-camel": ("transient", {"userName": "kdemo-g4-ua"}),
        "transient-lower": ("transient", {"username": "kdemo-g4-ua"}),
        "camel-shared-lower-transient": ("mixed", None),
    }
    for tag, (kind, kv) in variants.items():
        if kind == "mixed":
            src = 'nodeState.putShared("userName","kdemo-g4-ub");nodeState.putTransient("username","kdemo-g4-ua");action.goTo("true");'
        else:
            put = "putShared" if kind == "shared" else "putTransient"
            src = "".join(f'nodeState.{put}({json.dumps(k)},{json.dumps(v)});' for k, v in kv.items()) + 'action.goTo("true");'
        seed = fx.scripted(f"{tag}-seed", src, ["true"])
        ieu = fx.node("IdentifyExistingUserNode", IEU)
        nodes = {seed: ("ScriptedDecisionNode", "seed", {"true": ieu}), ieu: ("IdentifyExistingUserNode", "identify", {})}
        for oc in list_outcomes("IdentifyExistingUserNode", IEU):
            d = fx.scripted(f"{tag}-d{oc}", dump_source(KEYS, oc), ["true"])
            nodes[ieu][2][oc] = d
            nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
        t = fx.tree(tag, seed, nodes)
        r = authenticate(t)
        log(f"=== {tag}: {kind} {json.dumps(kv)}")
        show(tag, r)
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
