#!/usr/bin/env bash
# experiment-journey-session-property.sh
#
# Question: what does `action.putSessionProperty` / `removeSessionProperty` in a next-gen scripted
# decision node do to the session a successful journey creates, and what does it do with the
# names AM owns?
#
# Method: a tree whose first script identifies a throwaway user and puts/removes properties, then
# succeeds. A second tree, called with the first journey's session cookie, dumps `existingSession`
# for the probed keys. Cases:
#   two      put two properties in one chain
#   removed  put then remove the same key in one script
#   override a later script overwrites an earlier script's key; another script removes an earlier key
#   number   a number value
#   roundtrip put, then a later script waits on a callback, then success
#   owned-*  a property AM owns (UserId, AuthLevel, Service, Locale)
#   late     UserId put before a node that waits on a callback: is the journey failed at the put or
#            only when the session is made?
# Objects are kdemo-g3-*, deleted on exit, deletion confirmed by a GET.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g3}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json, urllib.error, urllib.parse, urllib.request
from experiment_journey_lib import BASE, REALM, AUTHN, Fixture, log, safe, SUCCESS, FAILURE

fx = Fixture("sp")
KEYS = ["kdemoA", "kdemoB", "kdemoN", "UserId", "AuthLevel", "Service", "Locale"]


def authn_raw(tree, body=None, cookie=None):
    q = urllib.parse.urlencode({"authIndexType": "service", "authIndexValue": tree})
    h = {"Accept-API-Version": AUTHN, "Content-Type": "application/json"}
    if cookie:
        h["Cookie"] = cookie
    req = urllib.request.Request(f"{BASE}/am/json/realms/root/realms/{REALM}/authenticate?{q}",
                                 data=json.dumps(body or {}).encode(), method="POST", headers=h)
    try:
        with urllib.request.urlopen(req) as r:
            raw, st, hd = r.read(), r.status, r.headers
    except urllib.error.HTTPError as e:
        raw, st, hd = e.read(), e.code, e.headers
    try:
        b = json.loads(raw)
    except ValueError:
        b = raw.decode(errors="replace")
    return st, b, hd.get_all("Set-Cookie") or []


READER = None


def reader():
    global READER
    if READER:
        return READER
    src = ('var o = {}; var keys = %s; for (var i = 0; i < keys.length; i++) { var k = keys[i];'
           ' o[k] = (existingSession && existingSession[k] !== undefined) ? String(existingSession[k]) : "<absent>"; }'
           ' if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("sess", JSON.stringify(o)); } else { action.goTo("true"); }') % json.dumps(KEYS)
    s = fx.scripted("reader", src, ["true"])
    READER = fx.tree("reader", s, {s: ("ScriptedDecisionNode", "reader", {"true": SUCCESS})})
    return READER


def session_cookie(cookies):
    for c in cookies:
        nm, _, rest = c.partition("=")
        val = rest.partition(";")[0]
        if len(val) > 20 and nm != "amlbcookie":
            return f"{nm}={val}"
    return None


def case(tag, scripts, roundtrip=False):
    """scripts: list of JS bodies run in order; the first also identifies a user."""
    user = fx.name("u-" + tag)
    fx.idm_user({"userName": user, "givenName": "Zz", "sn": "Sp", "mail": user + "@example.com"})
    ids = []
    for i, body in enumerate(scripts):
        pre = f'nodeState.putShared("username", "{user}");' if i == 0 else ""
        ids.append(fx.scripted(f"{tag}-{i}", pre + body, ["true"]))
    nodes = {}
    chain = ids[:]
    if roundtrip:
        wait = fx.scripted(f"{tag}-wait", 'if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("w", "x"); } else { action.goTo("true"); }', ["true"])
        chain.append(wait)
    for i, nid in enumerate(chain):
        nxt = chain[i + 1] if i + 1 < len(chain) else SUCCESS
        nodes[nid] = ("ScriptedDecisionNode", f"s{i}", {"true": nxt})
    tree = fx.tree(tag, chain[0], nodes)
    log(f"=== {tag}")
    st, body, cookies = authn_raw(tree)
    if roundtrip and st == 200 and isinstance(body, dict) and "callbacks" in body:
        log("    round trip: callback sent, answering")
        st, body, cookies = authn_raw(tree, body)
    log(f"    authenticate HTTP {st} {json.dumps(safe(body))[:160] if st != 200 or 'tokenId' not in body else 'tokenId'}")
    cookie = session_cookie(cookies)
    if not cookie or st != 200:
        log("    no session cookie")
        return
    st2, b2, _ = authn_raw(reader(), cookie=cookie)
    vals = [o["value"] for c in (b2.get("callbacks") if isinstance(b2, dict) else []) or [] for o in c["output"] if o["name"] == "value"]
    log(f"    existingSession in a second journey: HTTP {st2} {vals}")


try:
    case("two", ['action.goTo("true").putSessionProperty("kdemoA", "1").putSessionProperty("kdemoB", "2");'])
    case("removed", ['action.putSessionProperty("kdemoA", "1"); action.removeSessionProperty("kdemoA"); action.goTo("true");'])
    case("override", ['action.putSessionProperty("kdemoA", "first"); action.putSessionProperty("kdemoB", "keep"); action.goTo("true");',
                      'action.putSessionProperty("kdemoA", "second"); action.removeSessionProperty("kdemoB"); action.goTo("true");'])
    case("number", ['action.putSessionProperty("kdemoN", 5); action.goTo("true");'])
    case("roundtrip", ['action.putSessionProperty("kdemoA", "kept"); action.goTo("true");'], roundtrip=True)
    for name in ("UserId", "AuthLevel", "Service", "Locale"):
        case("owned-" + name.lower(), [f'action.putSessionProperty("{name}", "x"); action.goTo("true");'])
    case("owned-late", ['action.putSessionProperty("UserId", "x"); action.goTo("true");'], roundtrip=True)
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        if "delete=404" not in line:
            log("   ", line)
PY
