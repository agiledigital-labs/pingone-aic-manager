#!/usr/bin/env bash
# experiment-journey-identity-nodes.sh
#
# Question this answers:
#   For Identify Existing User, Create Object and Patch Object: what are the
#   outcomes, which state keys does each read (username? objectAttributes in
#   shared or transient?), and what does each write back into state?
#
# Method:
#   One throwaway alpha_user (UUID _id) is created through IDM. Each variant is
#   a tree: seed script (writes exactly the keys under test) -> node under test
#   -> one dump node per outcome. The dump reports nodeState on its first pass
#   and again after its own callback round trip (transient keys vanish).
#   Controls: a non-existent user for Identify; a body missing required fields
#   for Create; a non-existent userName for Patch; and for every "reads key X"
#   claim a sibling variant that supplies the same value under a different key
#   or in the other state channel. Patch results are read back from IDM.
#
# Every tree, script and user is named zzemu-* and deleted on exit.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
import urllib.parse
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, call, list_outcomes, REALM)

KEYS = ["username", "_id", "objectAttributes", "userName", "mail", "zzSeeded"]
USERS = f"/openidm/managed/{REALM}_user"
fx = Fixture("ident")


def seed_src(shared, transient):
    lines = ['nodeState.putShared("zzSeeded", "yes");']
    for k, v in shared.items():
        lines.append(f"nodeState.putShared({json.dumps(k)}, {json.dumps(v)});")
    for k, v in transient.items():
        lines.append(f"nodeState.putTransient({json.dumps(k)}, {json.dumps(v)});")
    lines.append('action.goTo("true");')
    return "\n".join(lines)


def run(tag, node_type, config, shared, transient):
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
    log(f"=== {tag}: {node_type} outcomes={outcomes} shared={sorted(shared)} transient={sorted(transient)}")
    r1 = authenticate(tree)
    show(f"{tag} pass1", r1)
    if r1[0] == 200 and isinstance(r1[1], dict) and "callbacks" in r1[1]:
        show(f"{tag} pass2", authenticate(tree, r1[1]))


def idm_user_by_name(name):
    q = urllib.parse.quote(f'userName eq "{name}"')
    st, b, _ = call("GET", f"{USERS}?_queryFilter={q}&_fields=_id,userName,sn,givenName,mail",
                    api="resource=1.0")
    return b.get("result", []) if isinstance(b, dict) else []


try:
    uname = fx.name("user")
    mail = f"{uname}@example.com"
    uid = fx.idm_user({"userName": uname, "givenName": "Zz", "sn": "Emu", "mail": mail})
    log(f"--- created IDM user _id={uid} userName={uname}")

    ieu = {"identityAttribute": "mail", "identifier": "userName"}
    run("ieu-oa-mail", "IdentifyExistingUserNode", ieu, {"objectAttributes": {"mail": mail}}, {})
    run("ieu-username-mail", "IdentifyExistingUserNode", ieu, {"username": mail}, {})
    run("ieu-username-uname", "IdentifyExistingUserNode", ieu, {"username": uname}, {})
    run("ieu-oa-mail-transient", "IdentifyExistingUserNode", ieu, {}, {"objectAttributes": {"mail": mail}})
    run("ieu-missing", "IdentifyExistingUserNode", ieu,
        {"objectAttributes": {"mail": "zzemu-nobody@example.com"}, "username": "zzemu-nobody@example.com"}, {})
    ieu2 = {"identityAttribute": "userName", "identifier": "userName"}
    run("ieu2-username-uname", "IdentifyExistingUserNode", ieu2, {"username": uname}, {})

    co = {"identityResource": f"managed/{REALM}_user"}
    for tag, shared, transient in [
            ("co-shared", {"objectAttributes": {"userName": fx.name("c1"), "givenName": "Zz", "sn": "C1",
                                                "mail": fx.name("c1") + "@example.com"}}, {}),
            ("co-transient", {}, {"objectAttributes": {"userName": fx.name("c2"), "givenName": "Zz", "sn": "C2",
                                                       "mail": fx.name("c2") + "@example.com"}}),
            ("co-missing-required", {"objectAttributes": {"userName": fx.name("c3")}}, {})]:
        run(tag, "CreateObjectNode", co, shared, transient)
    for n in ("c1", "c2", "c3"):
        rows = idm_user_by_name(fx.name(n))
        log(f"--- IDM after create, userName={fx.name(n)}: {len(rows)} row(s)",
            json.dumps([{k: r.get(k) for k in ('sn',)} for r in rows]))
        for r in rows:
            fx.track("user", f"{USERS}/{r['_id']}")

    po = {"identityResource": f"managed/{REALM}_user", "patchAsObject": False,
          "ignoredFields": [], "identityAttribute": "userName"}
    for tag, shared, transient in [
            ("po-shared", {"objectAttributes": {"userName": uname, "sn": "PatchedShared"}}, {}),
            ("po-transient", {}, {"objectAttributes": {"userName": uname, "sn": "PatchedTransient"}}),
            ("po-username-only", {"username": uname, "objectAttributes": {"sn": "PatchedByUsername"}}, {}),
            ("po-id-only", {"_id": uid, "objectAttributes": {"sn": "PatchedById"}}, {}),
            ("po-missing", {"objectAttributes": {"userName": "zzemu-nobody", "sn": "X"}}, {})]:
        run(tag, "PatchObjectNode", po, shared, transient)
        st, b, _ = call("GET", f"{USERS}/{uid}?_fields=sn,givenName", api="resource=1.0")
        log(f"--- IDM sn after {tag}: {b.get('sn') if isinstance(b, dict) else b}")
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
