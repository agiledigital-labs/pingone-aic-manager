#!/usr/bin/env bash
# experiment-journey-persistent-cookie.sh
#
# Question: what is the value SetPersistentCookieNode puts on the wire, and what does
# PersistentCookieDecisionNode need to accept one? The emulator round-trips its own cookie; this
# measures the real one's shape (header, nesting, claims where readable) and the node-config
# shapes of `hmacSigningKey` (plain, `{"$string": literal}`). The signing key is made up here
# (random, never an ESV); the cookie is minted by one tree and replayed against another.
#
# Objects are kdemo-g3-*, deleted on exit, deletion confirmed by a GET.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g3}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import base64, json, os, urllib.error, urllib.parse, urllib.request
from experiment_journey_lib import BASE, REALM, RP, AUTHN, Fixture, authenticate, call, log, safe, SUCCESS, FAILURE

fx = Fixture("pc")
KEY = base64.b64encode(os.urandom(32)).decode()
OTHER = base64.b64encode(os.urandom(32)).decode()


def b64d(part):
    return base64.urlsafe_b64decode(part + "=" * (-len(part) % 4))


def authn_raw(tree, body=None, cookie=None):
    """Like authenticate(), but keeps every Set-Cookie header."""
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


def shape(value):
    """Structure of a dotted value without its secrets: per part, length and decoded header if JSON."""
    out = []
    for i, part in enumerate(value.split(".")):
        item = {"part": i, "len": len(part)}
        try:
            raw = b64d(part)
            try:
                item["json"] = json.loads(raw)
            except ValueError:
                item["bytes"] = len(raw)
        except Exception:
            item["b64"] = False
        out.append(item)
    return out


def mint(label, name, key_cfg, extra=None):
    user = fx.name("u-" + label)
    fx.idm_user({"userName": user, "givenName": "Zz", "sn": "Pc", "mail": user + "@example.com"})
    setter = fx.scripted("set-" + label, f'nodeState.putShared("username", "{user}"); action.goTo("true");', ["true"])
    cfg = {"persistentCookieName": name, "hmacSigningKey": key_cfg, "maxLife": 2, "idleTimeout": 1,
           "useSecureCookie": True, "useHttpOnlyCookie": True, "sameSite": "LAX"}
    cfg.update(extra or {})
    pc = fx.node("SetPersistentCookieNode", cfg)
    tree = fx.tree("mint-" + label, setter, {
        setter: ("ScriptedDecisionNode", "set", {"true": pc}),
        pc: ("SetPersistentCookieNode", "pc", {"outcome": SUCCESS})})
    return user, tree


import hashlib, hmac


def sign(header, payload_b64, key_b64):
    signing = f"{header}.{payload_b64}"
    mac = hmac.new(base64.b64decode(key_b64), signing.encode(), hashlib.sha256).digest()
    return signing + "." + base64.urlsafe_b64encode(mac).rstrip(b"=").decode()


def b64e(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


COUNT = [0]


def decision_tree(name, key_cfg, extra=None):
    COUNT[0] += 1
    n = COUNT[0]
    cfg = {"persistentCookieName": name, "hmacSigningKey": key_cfg, "idleTimeout": 0, "enforceClientIp": False,
           "useSecureCookie": True, "useHttpOnlyCookie": True}
    cfg.update(extra or {})
    pcd = fx.node("PersistentCookieDecisionNode", cfg)
    src = 'if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("u", "%s:" + String(nodeState.get("username"))); } else { action.goTo("true"); }'
    yes = fx.scripted(f"dy{n}", src % "T", ["true"])
    no = fx.scripted(f"dn{n}", src % "F", ["true"])
    return fx.tree(f"dec{n}", pcd, {
        pcd: ("PersistentCookieDecisionNode", "pcd", {"true": yes, "false": no}),
        yes: ("ScriptedDecisionNode", "yes", {"true": SUCCESS}),
        no: ("ScriptedDecisionNode", "no", {"true": FAILURE})})


def replay(label, tree, name, value):
    st, body, _ = authn_raw(tree, cookie=f"{name}={value}")
    if isinstance(body, dict) and "callbacks" in body:
        u = [o["value"] for c in body["callbacks"] for o in c["output"] if o["name"] == "value"]
        res = f"decision {u}"
    else:
        res = json.dumps(safe(body))[:200]
    log(f"--- replay {label}: HTTP {st} {res}")


try:
    minted = {}
    for label, key_cfg in (("plain", KEY), ("wrapped", {"$string": KEY})):
        user, tree = mint(label, f"kdemo-g3-pc-{label}", key_cfg)
        st, body, cookies = authn_raw(tree)
        log(f"=== mint {label}: authenticate HTTP {st}; Set-Cookie headers: {len(cookies)}")
        for c in cookies:
            nm, _, rest = c.partition("=")
            val, _, attrs = rest.partition(";")
            if nm == f"kdemo-g3-pc-{label}":
                log(f"    cookie value-len={len(val)} attrs={attrs.strip()}")
                log("    shape:", json.dumps(shape(val)))
                inner = b64d(val.split(".")[1]).decode()
                log("    inner shape:", json.dumps(shape(inner)))
                minted[label] = (nm, val, user, key_cfg)
    nm, val, user, _ = minted["plain"]
    nm2, val2, user2, _ = minted["wrapped"]
    uid = call("GET", f"/openidm/managed/{REALM}_user?_queryFilter=" + urllib.parse.quote(f'userName eq "{user}"') + "&_fields=_id", api="resource=1.0")[1]["result"][0]["_id"]
    log(f"--- plain cookie's user _id differs from its name: {uid != user}")
    dtree = decision_tree(nm, KEY)
    replay("real cookie, right key (plain minted)", dtree, nm, val)
    replay("other cookie name sent", dtree, "kdemo-g3-pc-zzz", val)
    wrongtree = decision_tree(nm, OTHER, {"persistentCookieName": nm})
    replay("real cookie, node holds another key", wrongtree, nm, val)
    parts = val.split(".")
    hdr = parts[0]
    replay("outer re-signed with the same key (control: still true)", dtree, nm, sign(hdr, parts[1], KEY))
    inner_other = b64d(val2.split(".")[1]).decode()
    swapped_payload = b64e(inner_other.encode())
    replay("outer re-signed, inner taken from the other user's cookie (same key)", dtree, nm, sign(hdr, swapped_payload, KEY))
    ip = inner_other.split(".")
    flipped = ".".join(ip[:3] + [b64e(b64d(ip[3])[:-1] + bytes([b64d(ip[3])[-1] ^ 1]))] + ip[4:])
    replay("outer re-signed, one inner ciphertext byte flipped", dtree, nm, sign(hdr, b64e(flipped.encode()), KEY))
    claims = b64e(json.dumps({"sub": user, "exp": 4102444800, "iat": 1}).encode())
    replay("hand-signed: outer payload is plain JSON claims", dtree, nm, sign(hdr, claims, KEY))
    replay("hand-signed: header without cty, plain JSON claims", dtree, nm, sign(b64e(b'{"typ":"JWT","alg":"HS256"}'), claims, KEY))
    replay("hand-signed: outer payload is not base64 JSON", dtree, nm, sign(hdr, b64e(b"not.a.jwe.at.all"), KEY))
    replay("truncated: two parts", dtree, nm, ".".join(parts[:2]))
    replay("an empty cookie value", dtree, nm, "")
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
