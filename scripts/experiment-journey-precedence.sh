#!/usr/bin/env bash
# experiment-journey-precedence.sh
#
# Questions this answers:
#   1. Identify Existing User (identityAttribute "mail", identifier "userName"):
#      when shared `username` and `objectAttributes.mail` disagree, which source
#      is looked up, and does a miss on one fall back to the other?
#   2. `openidm.create` from a next-gen journey script with a content key whose
#      value is `undefined`: does the create succeed, and is the key absent,
#      null, or the string "undefined" on re-read?
#   3. `httpClient.send` with `body: undefined` at top level, and with an integer
#      in the body.
#
# Method:
#   1. Two throwaway alpha_users A and B (UUID _id). Each variant is a tree:
#      seed script (putShared of exactly the keys under test) -> Identify ->
#      one dump per outcome. Control: the agreeing seed (both name A). Then the
#      realistic shape, Page[UsernameCollector, AttributeCollector(mail)] ->
#      Identify, answered with agreeing and with disagreeing values.
#   2. One scripted node creates three users with caller-chosen UUIDs: content
#      without the key (control), with `description: undefined`, and with
#      `description: null`, and `description: "zz-set"`. Each is re-read over
#      IDM REST from outside AM, and a `description pr` query asks the store
#      which of the four hold a value at all ("zz-set" is its positive control:
#      a plain GET renders every schema property, so absent and null look alike).
#   3. The same echo method as docs/api/12 (httpbin.org/post, which returns the
#      raw request body as `data`): body omitted (control), `undefined`, `null`,
#      and `{count: 1}` next to its local JSON.stringify. Synthetic scalars only.
#
# Every tree, node, script and user is named/created zzemu-* and deleted on exit.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
import urllib.parse
import uuid
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, call, list_outcomes, REALM, hidden)

KEYS = ["username", "_id", "objectAttributes"]
USERS = f"/openidm/managed/{REALM}_user"
IEU = {"identityAttribute": "mail", "identifier": "userName"}
fx = Fixture("prec")


def seed_src(shared):
    lines = [f"nodeState.putShared({json.dumps(k)}, {json.dumps(v)});" for k, v in shared.items()]
    lines.append('action.goTo("true");')
    return "\n".join(lines)


def ieu_with_dumps(tag, nodes):
    nid = fx.node("IdentifyExistingUserNode", IEU)
    nodes[nid] = ("IdentifyExistingUserNode", "identify", {})
    for oc in list_outcomes("IdentifyExistingUserNode", IEU):
        d = fx.scripted(f"{tag}-d{oc}", dump_source(KEYS, oc), ["true"])
        nodes[nid][2][oc] = d
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
    return nid


def run_seed(tag, shared):
    seed = fx.scripted(f"{tag}-seed", seed_src(shared), ["true"])
    nodes = {seed: ("ScriptedDecisionNode", "seed", {})}
    nodes[seed][2]["true"] = ieu_with_dumps(tag, nodes)
    tree = fx.tree(tag, seed, nodes)
    log(f"=== {tag}: shared={json.dumps(shared)}")
    show(f"{tag} pass1", authenticate(tree))


def answer(body, vals):
    body = json.loads(json.dumps(body))
    for c in body.get("callbacks", []):
        out = {o["name"]: o.get("value") for o in c.get("output", [])}
        key = out.get("name") or c["type"]
        if key in vals and c.get("input"):
            c["input"][0]["value"] = vals[key]
    return body


def run_page(tag, uname, mail):
    u = fx.node("UsernameCollectorNode", {})
    ac = fx.node("AttributeCollectorNode", {"attributesToCollect": ["mail"], "identityAttribute": "userName",
                                            "validateInputs": False, "required": False})
    pg = fx.node("PageNode", {"nodes": [
        {"_id": u, "nodeType": "UsernameCollectorNode", "displayName": "zzemu user"},
        {"_id": ac, "nodeType": "AttributeCollectorNode", "displayName": "zzemu mail"}],
        "pageHeader": {}, "pageDescription": {}})
    nodes = {pg: ("PageNode", "page", {})}
    nodes[pg][2]["outcome"] = ieu_with_dumps(tag, nodes)
    tree = fx.tree(tag, pg, nodes)
    log(f"=== {tag}: page answered username={uname} mail={mail}")
    r = authenticate(tree)
    show(f"{tag} page", r)
    if r[0] == 200 and "callbacks" in r[1]:
        show(f"{tag} submit", authenticate(tree, answer(r[1], {"NameCallback": uname, "mail": mail})))


CREATE_SRC = r"""
if (callbacks.isEmpty()) {
  var ids = IDS;
  var base = function (n) {
    return { userName: "UNAME-" + n, givenName: "Zz", sn: "Emu", mail: "UNAME-" + n + "@example.com" };
  };
  var variants = {
    omitted: base("omitted"),
    undef: (function () { var o = base("undef"); o.description = undefined; return o; })(),
    nul: (function () { var o = base("nul"); o.description = null; return o; })(),
    str: (function () { var o = base("str"); o.description = "zz-set"; return o; })()
  };
  var out = {};
  for (var k in variants) {
    try {
      var r = openidm.create("managed/alpha_user", ids[k], variants[k]);
      out[k] = { ok: true, hasKeyInLocal: Object.prototype.hasOwnProperty.call(variants[k], "description"),
                 returnedId: String(r._id) === ids[k],
                 returnedDescription: r.description === undefined ? "<undefined>" :
                   (r.description === null ? "<null>" : "str:" + String(r.description)) };
    } catch (e) {
      out[k] = { ok: false, error: String(e) };
    }
  }
  callbacksBuilder.hiddenValueCallback("create", JSON.stringify(out));
} else {
  action.goTo("true");
}
"""

HTTP_SRC = r"""
if (callbacks.isEmpty()) {
  var URL = "https://httpbin.org/post";
  var out = {};
  var cases = {
    omitted: { method: "POST", headers: { "Content-Type": "application/json" } },
    undef: { method: "POST", headers: { "Content-Type": "application/json" }, body: undefined },
    nul: { method: "POST", headers: { "Content-Type": "application/json" }, body: null },
    count: { method: "POST", headers: { "Content-Type": "application/json" }, body: { count: 1 } }
  };
  out.countLocalStringify = JSON.stringify({ count: 1 });
  for (var k in cases) {
    try {
      var resp = httpClient.send(URL, cases[k]).get();
      var p = JSON.parse(resp.text());
      out[k] = { status: resp.status, data: p.data,
                 contentLength: p.headers ? (p.headers["Content-Length"] || "<none>") : "?",
                 contentType: p.headers ? (p.headers["Content-Type"] || "<none>") : "?" };
    } catch (e) {
      out[k] = { error: String(e) };
    }
  }
  callbacksBuilder.hiddenValueCallback("http", JSON.stringify(out));
} else {
  action.goTo("true");
}
"""


def one_node_tree(tag, src):
    s = fx.scripted(tag, src, ["true"])
    return fx.tree(tag, s, {s: ("ScriptedDecisionNode", tag, {"true": SUCCESS})})


try:
    a_name, b_name = fx.name("ua"), fx.name("ub")
    a_mail, b_mail = f"{a_name}@example.com", f"{b_name}@example.com"
    a_id = fx.idm_user({"userName": a_name, "givenName": "Zz", "sn": "A", "mail": a_mail})
    b_id = fx.idm_user({"userName": b_name, "givenName": "Zz", "sn": "B", "mail": b_mail})
    log(f"--- users: A _id={a_id} userName={a_name} mail={a_mail}")
    log(f"--- users: B _id={b_id} userName={b_name} mail={b_mail}")
    nobody = "zzemu-nobody@example.com"

    # Q1: seeded state
    run_seed("agree", {"username": a_name, "objectAttributes": {"mail": a_mail}})
    run_seed("oaB-unameA", {"username": a_name, "objectAttributes": {"mail": b_mail}})
    run_seed("oaB-umailA", {"username": a_mail, "objectAttributes": {"mail": b_mail}})
    run_seed("oaB-unobody", {"username": nobody, "objectAttributes": {"mail": b_mail}})
    run_seed("oanobody-unameA", {"username": a_name, "objectAttributes": {"mail": nobody}})
    run_seed("oanobody-umailA", {"username": a_mail, "objectAttributes": {"mail": nobody}})
    # Q1: realistic page shape
    run_page("page-agree", a_name, a_mail)
    run_page("page-disagree", a_name, b_mail)

    # Q2: openidm.create with an undefined value
    ids = {k: str(uuid.uuid4()) for k in ("omitted", "undef", "nul", "str")}
    for v in ids.values():
        fx.track("user", f"{USERS}/{v}")
    tree = one_node_tree("create", CREATE_SRC.replace("IDS", json.dumps(ids))
                         .replace("UNAME", fx.name("cr")))
    r = authenticate(tree)
    log("=== create", r[0], hidden(r[1], "create") if isinstance(r[1], dict) else json.dumps(r[1]))
    for k, v in ids.items():
        st, b, _ = call("GET", f"{USERS}/{v}", api="resource=1.0")
        if isinstance(b, dict) and st == 200:
            log(f"--- REST re-read {k}: HTTP {st} 'description' in body={'description' in b}"
                f" value={json.dumps(b.get('description', '<absent>'))}")
        else:
            log(f"--- REST re-read {k}: HTTP {st}")
        st, b, _ = call("GET", f"{USERS}/{v}?_fields=description", api="resource=1.0")
        log(f"--- REST re-read {k} _fields=description: HTTP {st} {json.dumps(b) if st != 200 else json.dumps({x: b.get(x, '<absent>') for x in ('description',)})}")

    # REST renders every schema property, so absent and null both read as
    # null; a presence query asks the store instead (str is its positive control).
    cr = fx.name("cr")
    for flt in (f'userName sw "{cr}" and description pr', f'userName sw "{cr}" and description eq "undefined"'):
        q = urllib.parse.quote(flt)
        st, b, _ = call("GET", f"{USERS}?_queryFilter={q}&_fields=userName", api="resource=1.0")
        rows = sorted(r.get("userName", "?").rsplit("-", 1)[-1] for r in b.get("result", [])) if isinstance(b, dict) else b
        log(f"--- query [{flt.split(' and ', 1)[1]}]: HTTP {st} matched={json.dumps(rows)}")

    # Q3: httpClient body serialization
    tree = one_node_tree("http", HTTP_SRC)
    r = authenticate(tree)
    log("=== http", r[0], hidden(r[1], "http") if isinstance(r[1], dict) else json.dumps(r[1]))
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
