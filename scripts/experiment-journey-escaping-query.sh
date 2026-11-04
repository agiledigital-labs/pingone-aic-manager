#!/usr/bin/env bash
# experiment-journey-escaping-query.sh
#
# Questions this answers:
#   1. How does `httpClient.send` (next-gen journey script, Content-Type
#      application/json, object body) escape strings on the wire? Printable
#      ASCII 0x20-0x7e (is `/` written `\/`? is `<` escaped?), non-ASCII (`é`,
#      U+00A0, U+2028, U+FEFF, an astral emoji), control characters (`\n`, `\t`,
#      U+0001, U+001F, U+007F), a lone surrogate, a key holding `/`, `<` and
#      `é`, and a null value.
#   2. Does `eq` on alpha_user `mail` / `userName` ignore leading, trailing or
#      interior whitespace in the query value? Over IDM REST, `openidm.query`
#      from a journey script, and Identify Existing User.
#   3. Do `gt` / `lt` on `userName` compare case-insensitively?
#   4. Is `eq` case-insensitive on other alpha_user string fields too
#      (givenName, sn, description, frIndexedString1, frUnindexedString1)?
#
# Method:
#   1. The docs/api/12 echo: httpbin.org/post returns the raw request body as
#      `data` and its byte count as Content-Length. One body per case, so a
#      refusal of one value cannot hide the others. Control: the script's own
#      JSON.stringify of the same object, returned alongside. The Python side
#      prints both with every non-ASCII code point as \uXXXX and the UTF-8 bytes
#      of the wire body in hex. Payloads are synthetic.
#   2. One alpha_user (UUID _id) with all-lower-case zzemu values. Queries with
#      the exact value (control) and with a leading space, a trailing space and
#      a space after `@`. Identify is seeded with each value as
#      objectAttributes.mail (identityAttribute mail).
#   3. Stored `<p>alice`; `gt "<p>Alice"`, `ge "<p>ALICE"`, `lt "<p>B"`,
#      `lt "<p>b"`. A case-sensitive (code-point) comparison and a
#      case-insensitive one disagree on the first three. Controls in the same
#      case: `gt "<p>alicd"` (match either way), `gt "<p>alice"` (no match
#      either way). Each filter is ANDed with `_id eq` and, separately, with a
#      `userName sw` prefix, so other users cannot match.
#   4. Each field stored with a distinct mixed-case value; `eq` with the
#      exact value (control) and with the case-swapped value.
#
# Every tree, node, script and user is zzemu-*, users have UUID _ids, and all are
# deleted on exit with the deletion confirmed by a GET.

set -uo pipefail
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
import urllib.parse
from experiment_journey_lib import (Fixture, authenticate, show, dump_source, log,
                                    SUCCESS, call, list_outcomes, hidden)

R = "resource=1.0"
USERS = "/openidm/managed/alpha_user"
fx = Fixture("esc")
KEYS = ["username", "_id", "objectAttributes"]
IEU = {"identityAttribute": "mail", "identifier": "userName"}


def esc(s):
    """ASCII-only rendering: a char outside 0x20-0x7e as <U+XXXX>, so a literal
    backslash escape on the wire stays distinguishable from the raw char."""
    if not isinstance(s, str):
        return json.dumps(s)
    return "".join(c if 0x20 <= ord(c) < 0x7f else f"<U+{ord(c):04X}>" for c in s)


def wire_bytes(d):
    """httpbin returns a body it will not render as text as a base64 data: URL."""
    pre = "data:application/octet-stream;base64,"
    if d.startswith(pre):
        import base64
        return base64.b64decode(d[len(pre):]), "base64 data: URL"
    return d.encode("utf-8", "surrogatepass"), "text"


def nonascii_hex(b):
    """Hex of every byte outside 0x20-0x7e, with its offset."""
    return " ".join(f"@{i}:{x:02x}" for i, x in enumerate(b) if not 0x20 <= x < 0x7f) or "<none>"


HTTP_SRC = r"""
if (callbacks.isEmpty()) {
  var URL = "https://httpbin.org/post";
  var ascii = "";
  for (var c = 0x20; c <= 0x7e; c++) { ascii += String.fromCharCode(c); }
  var key = "k/<é";
  var all = { ascii: ascii, eacute: "\u00e9", nbsp: "\u00a0", ls: "\u2028", bom: "\ufeff",
              emoji: "\ud83d\ude00", nl: "\n", tab: "\t", c01: "\u0001", c1f: "\u001f",
              del: "\u007f", nul: null };
  var k1 = {}; k1[key] = "x";
  // One body for everything that cannot fail, then the two that might, so a
  // refusal cannot hide the rest and httpbin sees few requests (it 429s bursts).
  var cases = { all: all, key: k1, lone: { v: "\ud800" } };
  var out = {};
  for (var k in cases) {
    var r = { local: JSON.stringify(cases[k]) };
    try {
      var resp = null;
      for (var attempt = 0; attempt < 5; attempt++) {
        resp = httpClient.send(URL, { method: "POST",
          headers: { "Content-Type": "application/json" }, body: cases[k] }).get();
        if (resp.status !== 429) { break; }
        var until = Date.now() + 2000;
        while (Date.now() < until) { /* back off: httpbin rate-limits bursts */ }
      }
      r.attempts = attempt + 1;
      var p = JSON.parse(resp.text());
      r.status = resp.status;
      r.data = p.data;
      r.len = p.headers ? (p.headers["Content-Length"] || "<none>") : "?";
    } catch (e) {
      r.error = String(e).slice(0, 300);
    }
    out[k] = r;
  }
  callbacksBuilder.hiddenValueCallback("http", JSON.stringify(out));
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


def one_node_tree(tag, src):
    s = fx.scripted(tag, src, ["true"])
    return fx.tree(tag, s, {s: ("ScriptedDecisionNode", tag, {"true": SUCCESS})})


def rest_count(flt):
    st, b, _ = call("GET", f"{USERS}?_queryFilter={urllib.parse.quote(flt)}&_fields=_id", api=R)
    return st, (len(b.get("result", [])) if isinstance(b, dict) and "result" in b else json.dumps(b)[:200])


def run_queries(label, queries, script=True):
    for k, flt in queries.items():
        st, n = rest_count(flt)
        log(f"--- REST {label} {k} [{esc(flt)}]: HTTP {st} resultCount={n}")
    if script:
        r = authenticate(one_node_tree(f"q-{label}", QUERY_SRC.replace("QUERIES", json.dumps(queries))))
        log(f"=== openidm.query {label}", r[0], hidden(r[1], "query") if isinstance(r[1], dict) else json.dumps(r[1]))


def ieu_with_dumps(tag, nodes):
    nid = fx.node("IdentifyExistingUserNode", IEU)
    nodes[nid] = ("IdentifyExistingUserNode", "identify", {})
    for oc in list_outcomes("IdentifyExistingUserNode", IEU):
        d = fx.scripted(f"{tag}-d{oc}", dump_source(KEYS, oc), ["true"])
        nodes[nid][2][oc] = d
        nodes[d] = ("ScriptedDecisionNode", f"dump {oc}", {"true": SUCCESS})
    return nid


def run_seed(tag, mail):
    src = f"nodeState.putShared(\"objectAttributes\", {json.dumps({'mail': mail})});\naction.goTo(\"true\");"
    seed = fx.scripted(f"{tag}-seed", src, ["true"])
    nodes = {seed: ("ScriptedDecisionNode", "seed", {})}
    nodes[seed][2]["true"] = ieu_with_dumps(tag, nodes)
    tree = fx.tree(tag, seed, nodes)
    log(f"=== IEU {tag}: objectAttributes.mail=[{esc(mail)}]")
    show(tag, authenticate(tree))


try:
    # ---------- Q1 ----------
    r = authenticate(one_node_tree("http", HTTP_SRC))
    res = json.loads(hidden(r[1], "http")) if isinstance(r[1], dict) and hidden(r[1], "http") else None
    if res is None:
        log("=== httpClient probe failed", r[0], json.dumps(r[1])[:400])
    else:
        for k, v in res.items():
            log(f"=== http {k}: status={v.get('status')} attempts={v.get('attempts')} Content-Length={v.get('len')} error={v.get('error', '-')}")
            log(f"    local JSON.stringify: {esc(v['local'])}")
            if "data" in v:
                b, how = wire_bytes(v["data"])
                log(f"    wire body ({how}): {esc(b.decode('utf-8', 'surrogateescape'))}")
                log(f"    wire bytes={len(b)} outside 0x20-0x7e (hex): {nonascii_hex(b)}")
                log(f"    identical to local : {b == v['local'].encode('utf-8', 'surrogatepass')}")

    # ---------- fixtures ----------
    p = fx.name("u")
    name, mail = f"{p}-alice", f"{p}@example.com"
    fields = {"givenName": f"Zzgiv{fx.run}", "sn": f"Zzsn{fx.run}", "description": f"Zzdesc{fx.run}",
              "frIndexedString1": f"Zzidx{fx.run}", "frUnindexedString1": f"Zzunx{fx.run}"}
    uid = fx.idm_user(dict({"userName": name, "mail": mail}, **fields))
    log(f"--- user _id={uid} userName={name} mail={mail}")
    st, b, _ = call("GET", f"{USERS}/{uid}?_fields=userName,mail," + ",".join(fields), api=R)
    log(f"--- re-read: HTTP {st} {json.dumps({k: b.get(k) for k in ['userName', 'mail', *fields]})}")

    # ---------- Q2 ----------
    at = mail.index("@")
    ws = {"exact": mail, "lead": " " + mail, "trail": mail + " ", "inner": mail[:at + 1] + " " + mail[at + 1:]}
    wsn = {"exact": name, "lead": " " + name, "trail": name + " ", "inner": name[:3] + " " + name[3:]}
    q2 = {f"mail-{k}": f'mail eq "{v}"' for k, v in ws.items()}
    q2.update({f"userName-{k}": f'userName eq "{v}"' for k, v in wsn.items()})
    run_queries("ws", q2)
    for k, v in ws.items():
        run_seed(f"ieu-{k}", v)

    # ---------- Q3 ----------
    pre = f"{p}-"
    q3 = {}
    for op, val in (("gt", "Alice"), ("ge", "ALICE"), ("lt", "B"), ("lt", "b"),
                    ("gt", "alicd"), ("gt", "alice"), ("ge", "alice")):
        q3[f"{op}-{val}-byid"] = f'_id eq "{uid}" and userName {op} "{pre}{val}"'
        q3[f"{op}-{val}-bysw"] = f'userName sw "{p}" and userName {op} "{pre}{val}"'
    run_queries("order", q3)

    # ---------- Q4 ----------
    q4 = {}
    for f, v in fields.items():
        q4[f"{f}-exact"] = f'{f} eq "{v}"'
        q4[f"{f}-swapped"] = f'{f} eq "{v.swapcase()}"'
    run_queries("case", q4, script=False)
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
