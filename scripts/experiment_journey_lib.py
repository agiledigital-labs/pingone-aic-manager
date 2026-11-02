"""Shared helpers for scripts/experiment-journey-*.sh.

Builds throwaway `zzemu` trees in realm alpha, drives /json/authenticate,
and deletes everything it created. Reads TENANT_BASE_URL and AIC_REPO from the
environment (set by the calling .sh); the bearer comes from the aic agent and is
never printed. Session tokens are reported only as their length.
"""
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

BASE = os.environ["TENANT_BASE_URL"].rstrip("/")
REPO = os.environ.get("AIC_REPO", ".")
REALM = "alpha"
RP = f"/am/json/realms/root/realms/{REALM}"
NODES = f"{RP}/realm-config/authentication/authenticationtrees/nodes"
TREES = f"{RP}/realm-config/authentication/authenticationtrees/trees"
SUCCESS = "70e691a5-1e33-4ac3-a356-e7b6d60d92e0"
FAILURE = "e301438c-0bd0-429c-ab0c-66126501069a"
CFG = "protocol=2.0,resource=1.0"
AUTHN = "protocol=1.0,resource=2.1"

_tok = {"v": None, "at": 0.0}


def token():
    if _tok["v"] is None or time.time() - _tok["at"] > 600:
        out = subprocess.run(["aic", "--no-prompt", "whoami", "--token"],
                             cwd=REPO, capture_output=True, text=True)
        if out.returncode != 0 or not out.stdout.strip():
            sys.exit("error: no bearer from the aic agent (locked?)")
        _tok["v"] = out.stdout.strip()
        _tok["at"] = time.time()
    return _tok["v"]


def call(method, path, body=None, api=CFG, auth=True, headers=None):
    """Return (status, parsed-or-text body, response headers)."""
    h = {"Accept-API-Version": api, "Content-Type": "application/json"}
    if auth:
        h["Authorization"] = "Bearer " + token()
    h.update(headers or {})
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, method=method, headers=h)
    try:
        with urllib.request.urlopen(req) as r:
            raw, status, rh = r.read(), r.status, dict(r.headers)
    except urllib.error.HTTPError as e:
        raw, status, rh = e.read(), e.code, dict(e.headers)
    if not raw:
        return status, "<empty body>", rh
    try:
        return status, json.loads(raw), rh
    except ValueError:
        return status, raw.decode(errors="replace"), rh


def b64(s):
    import base64
    return base64.b64encode(s.encode()).decode()


class Fixture:
    """Track created objects; delete in reverse on exit."""

    def __init__(self, label):
        self.label = label
        self.created = []  # (kind, path)
        self.run = uuid.uuid4().hex[:8]

    def name(self, suffix):
        return f"zzemu-{self.label}-{self.run}-{suffix}"

    def put(self, kind, path, body):
        st, b, _ = call("PUT", path, body)
        if st not in (200, 201):
            raise RuntimeError(f"PUT {kind} {path} -> {st}: {json.dumps(b)[:600]}")
        self.created.append((kind, path))
        return b

    def script(self, name_suffix, source):
        sid = str(uuid.uuid4())
        self.put("script", f"{RP}/scripts/{sid}", {
            "_id": sid, "name": self.name(name_suffix),
            "description": "zzemu journey runtime probe. Safe to delete.",
            "script": b64(source), "default": False, "language": "JAVASCRIPT",
            "context": "AUTHENTICATION_TREE_DECISION_NODE", "evaluatorVersion": "2.0",
        })
        return sid

    def node(self, node_type, body, nid=None):
        nid = nid or str(uuid.uuid4())
        self.put("node", f"{NODES}/{node_type}/{nid}", body)
        return nid

    def scripted(self, suffix, source, outcomes):
        sid = self.script(suffix, source)
        return self.node("ScriptedDecisionNode", {
            "script": sid, "outcomes": outcomes, "outputs": ["*"], "inputs": ["*"]})

    def tree(self, suffix, entry, nodes, inner_only=False):
        """nodes: {id: (nodeType, displayName, {outcome: target})}"""
        name = self.name(suffix)
        tn = {}
        for i, (nid, (ntype, disp, conns)) in enumerate(nodes.items()):
            tn[nid] = {"connections": conns, "displayName": f"zzemu {disp}",
                       "nodeType": ntype, "version": "1.0", "x": 100 + 150 * i, "y": 100}
        self.put("tree", f"{TREES}/{urllib.parse.quote(name)}", {
            "entryNodeId": entry, "nodes": tn, "enabled": True,
            "innerTreeOnly": inner_only, "noSession": False, "mustRun": False,
            "transactionalOnly": False,
            "identityResource": f"managed/{REALM}_user",
            "description": "zzemu journey runtime probe. Safe to delete.",
            "uiConfig": {"categories": "[]"},
        })
        return name

    def idm_user(self, attrs):
        uid = str(uuid.uuid4())
        st, b, _ = call("PUT", f"/openidm/managed/{REALM}_user/{uid}", attrs,
                        api="resource=1.0", headers={"If-None-Match": "*"})
        if st not in (200, 201):
            raise RuntimeError(f"create user -> {st}: {json.dumps(b)[:400]}")
        self.created.append(("user", f"/openidm/managed/{REALM}_user/{uid}"))
        return uid

    def track(self, kind, path):
        self.created.append((kind, path))

    def cleanup(self):
        report = []
        # trees first, then nodes, scripts, users
        order = {"tree": 0, "node": 1, "script": 2, "user": 3}
        for kind, path in sorted(reversed(self.created), key=lambda kp: order.get(kp[0], 9)):
            api = "resource=1.0" if kind == "user" else CFG
            st, _, _ = call("DELETE", path, api=api)
            st2, _, _ = call("GET", path, api=api)
            report.append(f"{kind} {path.rsplit('/', 1)[-1]} delete={st} get-after={st2}")
        return report


def authenticate(tree, body=None, params=None, headers=None):
    q = {"authIndexType": "service", "authIndexValue": tree}
    q.update(params or {})
    path = f"/am/json/realms/root/realms/{REALM}/authenticate?" + urllib.parse.urlencode(q)
    return call("POST", path, body, api=AUTHN, auth=False, headers=headers)


def list_outcomes(node_type, config):
    """POST nodes/{type}?_action=listOutcomes with a node config as the body."""
    st, b, _ = call("POST", f"{NODES}/{node_type}?_action=listOutcomes", config)
    if st != 200:
        raise RuntimeError(f"listOutcomes {node_type} -> {st}: {b}")
    return [o["id"] for o in b]


def safe(body):
    """Copy of an authenticate response with secrets reduced to lengths."""
    if not isinstance(body, dict):
        return body
    out = dict(body)
    for k in ("tokenId", "authId"):
        if k in out and isinstance(out[k], str):
            out[k] = f"<{k} len={len(out[k])}>"
    return out


def cbs(body):
    """Compact view of callbacks: [(type, outputs, input names)]."""
    res = []
    for c in (body or {}).get("callbacks", []) if isinstance(body, dict) else []:
        res.append({"type": c.get("type"),
                    "output": {o["name"]: o.get("value") for o in c.get("output", [])},
                    "input": [i["name"] for i in c.get("input", [])]})
    return res


def hidden(body, cid=None):
    """Values of HiddenValueCallbacks keyed by id."""
    out = {}
    for c in (body or {}).get("callbacks", []) if isinstance(body, dict) else []:
        if c.get("type") == "HiddenValueCallback":
            o = {x["name"]: x.get("value") for x in c.get("output", [])}
            out[o.get("id")] = o.get("value")
    return out if cid is None else out.get(cid)


def fill(body, values):
    """Set input[0].value of callbacks by index -> value."""
    body = json.loads(json.dumps(body))
    for idx, v in values.items():
        body["callbacks"][idx]["input"][0]["value"] = v
    return body


# Result-node script: dump named state keys into a HiddenValueCallback, then
# report through a TextOutput so the response is readable. Never dumps whole
# objects. KEYS is substituted.
DUMP = r"""
var keys = KEYS;
var out = {};
for (var i = 0; i < keys.length; i++) {
  var k = keys[i];
  var g = nodeState.get(k);
  var gs = typeof nodeState.getShared === "function" ? nodeState.getShared(k) : "n/a";
  var gt = typeof nodeState.getTransient === "function" ? nodeState.getTransient(k) : "n/a";
  if (k === "password") {
    // never echo a password; report presence and length only
    g = g === null ? null : "len=" + String(g).length;
    gs = gs === null || gs === "n/a" ? gs : "len=" + String(gs).length;
    gt = gt === null || gt === "n/a" ? gt : "len=" + String(gt).length;
  }
  out[k] = { get: String(g), shared: String(gs), transient: String(gt) };
}
var ks = [];
var it = nodeState.keys().iterator();
while (it.hasNext()) { ks.push(String(it.next())); }
out["__keys"] = ks.sort().join(",");
var m = [];
var names = ["get","getShared","getTransient","getObject","putShared","putTransient","mergeShared","mergeTransient","isDefined","remove","keys"];
for (var j = 0; j < names.length; j++) { m.push(names[j] + ":" + typeof nodeState[names[j]]); }
out["__methods"] = m.join(",");
var pass = callbacks.isEmpty() ? 1 : 2;
callbacksBuilder.hiddenValueCallback("dump-" + TAG + "-pass" + pass, JSON.stringify(out));
"""


def dump_source(keys, tag="x", extra=""):
    """A node that never completes: each pass emits its state snapshot as a
    HiddenValueCallback `dump-<tag>-pass<n>`. Pass 2 is the same node resumed
    after a callback round trip, so a key present on pass 1 and absent on pass 2
    was transient."""
    return extra + DUMP.replace("KEYS", json.dumps(keys)).replace("TAG", json.dumps(tag))


def show(label, resp):
    st, body, _ = resp
    log(f"--- {label}: HTTP {st}")
    if isinstance(body, dict) and "callbacks" in body:
        for c in cbs(body):
            if c["type"] == "HiddenValueCallback":
                log("   ", c["output"].get("id"), "=", c["output"].get("value"))
            else:
                log("   ", json.dumps(c))
        rest = {k: v for k, v in safe(body).items() if k != "callbacks"}
        log("    other keys:", json.dumps(rest))
    else:
        log("   ", json.dumps(safe(body)))


def log(*a):
    print(*a, flush=True)
