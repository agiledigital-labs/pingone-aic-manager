#!/usr/bin/env bash
# experiment-journey-create-nopassword.sh
#
# Question: a Create Object node on alpha_user whose objectAttributes carry no password: does the
# tenant create the user, and what does the user read back as inside a journey script for `password`
# (absent, null, a string) by openidm.read, with explicit fields, and by openidm.query, next to a user
# created with a password as the control?
#
# Method: (1) a tree whose seed script writes shared objectAttributes without a password, Create Object,
# then a dump; the user is then read from outside AM over IDM REST (200/404, whether `password` is a
# key). (2) a script creates two users with caller-chosen ids by openidm.create, one without a
# password (the case) and one with (the control), and reports for each of read, read with ["*"],
# read with ["password"], query, and a `password pr` filter as the control that it was stored: whether `password` is an own key, its typeof, whether it starts
# with the encoded prefix. Passwords are never printed. kdemo-g4-* users, deleted on exit.
set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g4}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
import uuid
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log, SUCCESS, call, REALM, list_outcomes)

USERS = f"/openidm/managed/{REALM}_user"
fx = Fixture("np")
NOPW, WITHPW = str(uuid.uuid4()), str(uuid.uuid4())

REPORT = r"""
if (callbacks.isEmpty()) {
  var ids = IDS;
  var res = {};
  var shape = function (o) {
    if (o === null || o === undefined) return "no-object";
    var has = Object.prototype.hasOwnProperty.call(o, "password") || (o.password !== undefined);
    var v = o.password;
    return { has: has, keys: Object.keys(o).length, inJson: JSON.stringify(o).indexOf('"password"') >= 0,
             typeof: v === null ? "null" : typeof v,
             encoded: typeof v === "string" ? String(v).indexOf("{PBKDF2-HMAC-SHA256}") === 0 : null };
  };
  var mk = function (id, pw) {
    var c = { userName: "kdemo-g4-" + id.substring(0, 8), givenName: "Zz", sn: "Emu", mail: "kdemo-g4-" + id.substring(0, 8) + "@example.com" };
    if (pw) c.password = pw;
    return c;
  };
  openidm.create("managed/alpha_user", ids.nopw, mk(ids.nopw, null));
  openidm.create("managed/alpha_user", ids.withpw, mk(ids.withpw, "Kdemo-g4-Pw-12345"));
  ["nopw", "withpw"].forEach(function (k) {
    var p = "managed/alpha_user/" + ids[k];
    var r = {};
    r.read = shape(openidm.read(p));
    r.readStar = shape(openidm.read(p, null, ["*"]));
    r.readPassword = shape(openidm.read(p, null, ["password"]));
    var q = openidm.query("managed/alpha_user", { _queryFilter: '_id eq "' + ids[k] + '"' });
    r.queryCount = q.result.length;
    r.query = shape(q.result[0]);
    var q2 = openidm.query("managed/alpha_user", { _queryFilter: '_id eq "' + ids[k] + '"', _fields: "password" });
    r.queryPassword = shape(q2.result[0]);
    // Control that a password was stored at all: a presence filter on it.
    try {
      r.prCount = openidm.query("managed/alpha_user", { _queryFilter: '_id eq "' + ids[k] + '" and password pr' }).result.length;
    } catch (e) { r.prCount = "throws:" + String(e).substring(0, 120); }
    res[k] = r;
  });
  callbacksBuilder.hiddenValueCallback("report", JSON.stringify(res));
}
action.goTo("true");
"""
CREATE_SEED = r"""
nodeState.putShared("objectAttributes", {userName: "kdemo-g4-created", givenName: "Zz", sn: "Emu", mail: "kdemo-g4-created@example.com"});
action.goTo("true");
"""
try:
    fx.track("user", f"{USERS}/{NOPW}")
    fx.track("user", f"{USERS}/{WITHPW}")
    rep = fx.scripted("report", REPORT.replace("IDS", json.dumps({"nopw": NOPW, "withpw": WITHPW})), ["true"])
    t = fx.tree("report", rep, {rep: ("ScriptedDecisionNode", "report", {"true": SUCCESS})})
    show("read shapes inside a journey script (no password vs password)", authenticate(t))

    seed = fx.scripted("seed", CREATE_SEED, ["true"])
    cr = fx.node("CreateObjectNode", {"identityResource": f"managed/{REALM}_user"})
    nodes = {seed: ("ScriptedDecisionNode", "seed", {"true": cr}), cr: ("CreateObjectNode", "create", {})}
    for oc in list_outcomes("CreateObjectNode", {"identityResource": f"managed/{REALM}_user"}):
        d = fx.scripted(f"d{oc}", dump_source(["username"], oc), ["true"])
        nodes[cr][2][oc] = d
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
    t2 = fx.tree("create", seed, nodes)
    show("Create Object without a password", authenticate(t2))
    st, b, _ = call("GET", f"{USERS}?_queryFilter=" + "userName+eq+%22kdemo-g4-created%22&_fields=userName,mail,password", api="resource=1.0")
    log("--- created user read over IDM REST:", st, json.dumps(b)[:300])
    for u in (b.get("result", []) if isinstance(b, dict) else []):
        fx.track("user", f"{USERS}/{u['_id']}")
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
