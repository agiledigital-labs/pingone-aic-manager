#!/usr/bin/env bash
# experiment-journey-datastore-status.sh
#
# Question: what does DataStoreDecisionNode do with a user whose `accountStatus` is "active",
# "Active", "inactive", "Inactive", "", null or absent (the right password in every case, and a
# wrong one for the unset user as a control)?
#
# Method: one throwaway alpha_user per status, each with the same password. A seed script puts the
# user's name in shared `username` and the password in transient `password`; the node's true and
# false outcomes lead to dump nodes that report which one was taken. Objects are kdemo-g3-*,
# deleted on exit, deletion confirmed by a GET.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g3}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log, SUCCESS, call, REALM)

fx = Fixture("dsd")
PASSWORD = "Kd3mo-Pass-9x!"


def seed_src(name, password):
    return (f'nodeState.putShared("username", {json.dumps(name)});'
            f'nodeState.putTransient("password", {json.dumps(password)});'
            'action.goTo("true");')


def run(tag, status, password=PASSWORD, set_status=True):
    attrs = {"userName": fx.name(f"u-{tag}"), "givenName": "Zz", "sn": "Emu",
             "mail": f"{fx.name(f'u-{tag}')}@example.com", "password": PASSWORD}
    if set_status:
        attrs["accountStatus"] = status
    fx.idm_user(attrs)
    st, body, _ = call("GET", f"/openidm/managed/{REALM}_user?_queryFilter=" +
                       f'userName+eq+"{attrs["userName"]}"&_fields=accountStatus', api="resource=1.0")
    stored = body["result"][0].get("accountStatus", "<absent>") if st == 200 and body.get("result") else "?"
    seed = fx.scripted(f"{tag}-seed", seed_src(attrs["userName"], password), ["true"])
    nid = fx.node("DataStoreDecisionNode", {})
    nodes = {seed: ("ScriptedDecisionNode", "seed", {"true": nid}),
             nid: ("DataStoreDecisionNode", "ds", {})}
    for oc in ("true", "false"):
        d = fx.scripted(f"{tag}-d{oc}", dump_source(["username"], oc.upper()), ["true"])
        nodes[nid][2][oc] = d
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
    tree = fx.tree(tag, seed, nodes)
    log(f"=== {tag}: requested accountStatus={status!r} set={set_status} stored={stored!r} password={'right' if password == PASSWORD else 'WRONG'}")
    show(f"{tag}", authenticate(tree))


try:
    run("active", "active")
    run("capital", "Active")
    run("inactive", "inactive")
    run("capinactive", "Inactive")
    run("empty", "")
    run("null", None)
    run("absent", None, set_status=False)
    run("absentwrong", None, password="not-the-password", set_status=False)
    run("inactivewrong", "inactive", password="not-the-password")
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
