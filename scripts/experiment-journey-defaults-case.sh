#!/usr/bin/env bash
# experiment-journey-defaults-case.sh
#
# Questions this answers:
#   1. `openidm.create` from a next-gen journey script with a content key whose
#      value is `undefined`, on a property that HAS a schema `default`: is the
#      key dropped (create succeeds, default applied) or sent as `null` (403
#      NOT_NULL, or null stored)? experiment-journey-precedence.sh could not tell,
#      because its property had no default.
#   2. AttributeCollectorNode with `required: false` answered with a blank
#      value: is `objectAttributes.mail` absent, `""` or `null`? And does Identify
#      Existing User (identityAttribute mail) then look up `""` or fall back to
#      the shared `username`?
#   3. Is `eq` on `mail` / `userName` case-sensitive — over IDM REST, through
#      `openidm.query` in a journey script, and through Identify Existing User?
#
# Method:
#   1. alpha_user has no property with a `default` in the live schema (checked at
#      run time, below); bravo_user's `frIndexedInteger1` has `default: 0`. No
#      schema is written: the probe creates bravo_users (UUID _id) with that
#      property omitted (control: the default lands as 0), `null` (control: the
#      doc-10 NOT_NULL refusal), `undefined`, and `7` (positive control). The
#      omitted and null controls are repeated over IDM REST.
#   2. Page[UsernameCollector, AttributeCollector(mail, required false)] -> a
#      callback-free probe node that records what it sees in objectAttributes ->
#      Identify -> one dump per outcome. The username is answered with the
#      user's MAIL, so a fallback to `username` would find the user (`true`)
#      while a lookup of `""` would not. Control: the same page with the mail
#      filled in. Seeded variants (`putShared` of `{mail: ""}` / `{mail: null}`)
#      separate what the collector writes from how Identify reads it.
#   3. One user with a lower-case mail and userName; queries with a mixed-case
#      value and (control) the exact value, over REST and from openidm.query;
#      then Identify seeded with a mixed-case and an exact objectAttributes.mail.
#
# Every tree, node, script and user is zzemu-*, users have UUID _ids, and all are
# deleted on exit with the deletion confirmed by a GET.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
import urllib.parse
import uuid
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, call, list_outcomes, hidden)

R = "resource=1.0"
fx = Fixture("dfc")
KEYS = ["username", "_id", "objectAttributes", "zzpre"]
IEU = {"identityAttribute": "mail", "identifier": "userName"}


def has_default(obj):
    st, b, _ = call("GET", "/openidm/config/managed", api=R)
    for o in b.get("objects", []):
        if o["name"] == obj:
            return sorted(k for k, p in o["schema"]["properties"].items() if "default" in p)
    return None


def answer(body, vals):
    body = json.loads(json.dumps(body))
    for c in body.get("callbacks", []):
        out = {o["name"]: o.get("value") for o in c.get("output", [])}
        key = out.get("name") or c["type"]
        if key in vals and c.get("input"):
            c["input"][0]["value"] = vals[key]
    return body


def ieu_with_dumps(tag, nodes, cfg=IEU):
    nid = fx.node("IdentifyExistingUserNode", cfg)
    nodes[nid] = ("IdentifyExistingUserNode", "identify", {})
    for oc in list_outcomes("IdentifyExistingUserNode", cfg):
        d = fx.scripted(f"{tag}-d{oc}", dump_source(KEYS, oc), ["true"])
        nodes[nid][2][oc] = d
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
    return nid


# Records, without a callback, what objectAttributes.mail looks like to a script.
PRE_SRC = r"""
var desc = function (m) {
  if (m === null || m === undefined) { return "<no objectAttributes>"; }
  var has = typeof m.containsKey === "function" ? m.containsKey("mail")
                                                : Object.prototype.hasOwnProperty.call(m, "mail");
  var v = typeof m.get === "function" ? m.get("mail") : m["mail"];
  return "hasMail=" + has + " typeof=" + typeof v + " isNull=" + (v === null) +
         " isEmpty=" + (v === "") + " value=" + JSON.stringify(v === undefined ? "<undefined>" : String(v));
};
// next-gen nodeState has no getShared/getTransient; get() is what a node sees.
nodeState.putShared("zzpre", desc(nodeState.get("objectAttributes")));
action.goTo("true");
"""


def run_page(tag, uname, mail, required=False):
    u = fx.node("UsernameCollectorNode", {})
    ac = fx.node("AttributeCollectorNode", {"attributesToCollect": ["mail"], "identityAttribute": "userName",
                                            "validateInputs": False, "required": required})
    pg = fx.node("PageNode", {"nodes": [
        {"_id": u, "nodeType": "UsernameCollectorNode", "displayName": "zzemu user"},
        {"_id": ac, "nodeType": "AttributeCollectorNode", "displayName": "zzemu mail"}],
        "pageHeader": {}, "pageDescription": {}})
    pre = fx.scripted(f"{tag}-pre", PRE_SRC, ["true"])
    nodes = {pg: ("PageNode", "page", {"outcome": pre}), pre: ("ScriptedDecisionNode", "pre", {})}
    nodes[pre][2]["true"] = ieu_with_dumps(tag, nodes)
    tree = fx.tree(tag, pg, nodes)
    log(f"=== {tag}: page answered username={uname} mail={json.dumps(mail)}")
    r = authenticate(tree)
    if r[0] == 200 and "callbacks" in r[1]:
        show(f"{tag} submit", authenticate(tree, answer(r[1], {"NameCallback": uname, "mail": mail})))
    else:
        show(f"{tag} page", r)


def run_seed(tag, shared, cfg=IEU):
    lines = [f"nodeState.putShared({json.dumps(k)}, {json.dumps(v)});" for k, v in shared.items()]
    seed = fx.scripted(f"{tag}-seed", "\n".join(lines) + "\n" + PRE_SRC, ["true"])
    nodes = {seed: ("ScriptedDecisionNode", "seed", {})}
    nodes[seed][2]["true"] = ieu_with_dumps(tag, nodes, cfg)
    tree = fx.tree(tag, seed, nodes)
    log(f"=== {tag}: shared={json.dumps(shared)} cfg={json.dumps(cfg)}")
    show(f"{tag}", authenticate(tree))


def one_node_tree(tag, src):
    s = fx.scripted(tag, src, ["true"])
    return fx.tree(tag, s, {s: ("ScriptedDecisionNode", tag, {"true": SUCCESS})})


CREATE_SRC = r"""
if (callbacks.isEmpty()) {
  var ids = IDS;
  var base = function (n) {
    return { userName: "UNAME-" + n, givenName: "Zz", sn: "Emu", mail: "UNAME-" + n + "@example.com" };
  };
  var mk = function (n, how) { var o = base(n); how(o); return o; };
  var variants = {
    omitted: base("omitted"),
    undef: mk("undef", function (o) { o.frIndexedInteger1 = undefined; }),
    nul: mk("nul", function (o) { o.frIndexedInteger1 = null; }),
    seven: mk("seven", function (o) { o.frIndexedInteger1 = 7; })
  };
  var out = {};
  for (var k in variants) {
    try {
      var r = openidm.create("managed/bravo_user", ids[k], variants[k]);
      var v = r.frIndexedInteger1;
      out[k] = { ok: true, returned: v === undefined ? "<undefined>" : (v === null ? "<null>" : String(v)) };
    } catch (e) {
      var d = "<none>";
      try {
        var je = e.javaException || e;
        if (je && typeof je.getDetail === "function") { d = String(je.getDetail()); }
        else if (je && typeof je.toJsonValue === "function") { d = String(je.toJsonValue()); }
      } catch (e2) { d = "<detail unreadable: " + String(e2).slice(0, 100) + ">"; }
      out[k] = { ok: false, error: String(e).slice(0, 300), detail: d.slice(0, 400) };
    }
  }
  callbacksBuilder.hiddenValueCallback("create", JSON.stringify(out));
} else {
  action.goTo("true");
}
"""

QUERY_SRC = r"""
if (callbacks.isEmpty()) {
  var q = QUERIES;
  var out = {};
  for (var k in q) {
    try {
      var r = openidm.query("managed/alpha_user", { _queryFilter: q[k] }, ["_id"]);
      out[k] = r.result.length;
    } catch (e) {
      out[k] = "error: " + String(e).slice(0, 200);
    }
  }
  callbacksBuilder.hiddenValueCallback("query", JSON.stringify(out));
} else {
  action.goTo("true");
}
"""


def mixed(s):
    return "".join(c.upper() if i % 2 == 0 else c.lower() for i, c in enumerate(s))


try:
    # ---------- Q1 ----------
    log("--- alpha_user properties with a schema default:", json.dumps(has_default("alpha_user")))
    log("--- bravo_user properties with a schema default:", json.dumps(has_default("bravo_user")))
    BU = "/openidm/managed/bravo_user"
    rest = {}
    for k, extra in (("rest-omitted", {}), ("rest-null", {"frIndexedInteger1": None})):
        uid = str(uuid.uuid4())
        nm = fx.name(k)
        fx.track("user", f"{BU}/{uid}")
        st, b, _ = call("PUT", f"{BU}/{uid}", dict({"userName": nm, "givenName": "Zz", "sn": "Emu",
                                                     "mail": nm + "@example.com"}, **extra),
                        api=R, headers={"If-None-Match": "*"})
        detail = b.get("frIndexedInteger1", "<absent>") if st in (200, 201) else json.dumps(b)[:300]
        log(f"--- REST create bravo_user {k}: HTTP {st} frIndexedInteger1={json.dumps(detail)}")
    ids = {k: str(uuid.uuid4()) for k in ("omitted", "undef", "nul", "seven")}
    for v in ids.values():
        fx.track("user", f"{BU}/{v}")
    tree = one_node_tree("create", CREATE_SRC.replace("IDS", json.dumps(ids)).replace("UNAME", fx.name("cr")))
    r = authenticate(tree)
    log("=== script create", r[0], hidden(r[1], "create") if isinstance(r[1], dict) else json.dumps(r[1]))
    for k, v in ids.items():
        st, b, _ = call("GET", f"{BU}/{v}?_fields=frIndexedInteger1", api=R)
        log(f"--- REST re-read {k}: HTTP {st} frIndexedInteger1="
            f"{json.dumps(b.get('frIndexedInteger1', '<absent>')) if st == 200 else '-'}")
    cr = fx.name("cr")
    for flt in (f'userName sw "{cr}" and frIndexedInteger1 eq 0', f'userName sw "{cr}" and frIndexedInteger1 pr'):
        st, b, _ = call("GET", f"{BU}?_queryFilter={urllib.parse.quote(flt)}&_fields=userName", api=R)
        rows = sorted(x.get("userName", "?").rsplit("-", 1)[-1] for x in b.get("result", [])) if isinstance(b, dict) else b
        log(f"--- query [{flt.split(' and ', 1)[1]}]: HTTP {st} matched={json.dumps(rows)}")

    # ---------- fixtures for Q2/Q3 ----------
    a_name = fx.name("ua")
    a_mail = f"{a_name}@example.com"
    a_id = fx.idm_user({"userName": a_name, "givenName": "Zz", "sn": "A", "mail": a_mail})
    log(f"--- user A _id={a_id} userName={a_name} mail={a_mail}")

    # ---------- Q2 ----------
    run_page("blank", a_mail, "")
    run_page("filled", a_mail, a_mail)
    run_seed("seed-empty", {"username": a_mail, "objectAttributes": {"mail": ""}})
    run_seed("seed-null", {"username": a_mail, "objectAttributes": {"mail": None}})
    run_seed("seed-nomail", {"username": a_mail, "objectAttributes": {"sn": "A"}})

    # ---------- Q3 ----------
    m_mail, m_name = mixed(a_mail), mixed(a_name)
    log(f"--- mixed-case mail={m_mail} userName={m_name}")
    queries = {"mail-exact": f'mail eq "{a_mail}"', "mail-mixed": f'mail eq "{m_mail}"',
               "userName-exact": f'userName eq "{a_name}"', "userName-mixed": f'userName eq "{m_name}"'}
    for k, flt in queries.items():
        st, b, _ = call("GET", f"/openidm/managed/alpha_user?_queryFilter={urllib.parse.quote(flt)}&_fields=_id", api=R)
        log(f"--- REST query {k}: HTTP {st} resultCount={len(b.get('result', [])) if isinstance(b, dict) else b}")
    tree = one_node_tree("query", QUERY_SRC.replace("QUERIES", json.dumps(queries)))
    r = authenticate(tree)
    log("=== openidm.query", r[0], hidden(r[1], "query") if isinstance(r[1], dict) else json.dumps(r[1]))
    run_seed("ieu-mail-exact", {"objectAttributes": {"mail": a_mail}})
    run_seed("ieu-mail-mixed", {"objectAttributes": {"mail": m_mail}})
    cfg_u = {"identityAttribute": "userName", "identifier": "userName"}
    run_seed("ieu-uname-exact", {"username": a_name}, cfg_u)
    run_seed("ieu-uname-mixed", {"username": m_name}, cfg_u)
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
