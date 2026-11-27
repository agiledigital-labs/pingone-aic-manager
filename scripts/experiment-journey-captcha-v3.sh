#!/usr/bin/env bash
# experiment-journey-captcha-v3.sh
#
# Question: what does product-CaptchaNode with `reCaptchaV3: true` send, which of its config keys
# reach the callback, and what happens to a token? Compared with the same node at `reCaptchaV3: false`.
#
# Method: nodes with invented site and secret keys (nothing is sent to a provider by this probe
# except what the node itself does on a token), standalone and as the last child of a Page after a
# username collector. Cases: first callback; an empty token; a fake token. Objects are kdemo-g3-*,
# deleted on exit, deletion confirmed by a GET.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g3}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json
from experiment_journey_lib import (Fixture, authenticate, log, safe, cbs, dump_source, call, NODES, list_outcomes, SUCCESS, FAILURE)

fx = Fixture("cap")


def cfg(v3, **extra):
    base = {"siteKey": "kdemo-site-key", "secretKey": "kdemo-secret-key", "apiUri": "https://www.google.com/recaptcha/api.js",
            "divClass": "g-recaptcha", "reCaptchaV3": v3, "disableSubmission": False, "scoreThreshold": "0.5"}
    base.update(extra)
    return base


def view(label, resp):
    st, body, _ = resp
    log(f"--- {label}: HTTP {st}")
    if not isinstance(body, dict) or "callbacks" not in body:
        log("   ", json.dumps(safe(body))[:300])
        return
    for c in cbs(body):
        log("   ", c["type"], json.dumps(c["output"])[:600], "inputs", json.dumps(c["input"]))


def tree_for(tag, config, in_page=False):
    nid = fx.node("product-CaptchaNode", config)
    st, stored, _ = call("GET", f"{NODES}/product-CaptchaNode/{nid}")
    log(f"=== {tag}: stored config (secretKey omitted): " + json.dumps({k: v for k, v in (stored or {}).items() if k not in ("secretKey", "_id", "_rev")}))
    log("    outcomes:", list_outcomes("product-CaptchaNode", config))
    ok = fx.scripted(f"{tag}-t", dump_source(["username"], "TRUE"), ["true"])
    no = fx.scripted(f"{tag}-f", dump_source(["username"], "FALSE"), ["true"])
    if in_page:
        un = fx.node("UsernameCollectorNode", {})
        pg = fx.node("PageNode", {"nodes": [{"_id": un, "nodeType": "UsernameCollectorNode", "displayName": "u"},
                                           {"_id": nid, "nodeType": "product-CaptchaNode", "displayName": "c"}],
                                  "pageHeader": {}, "pageDescription": {}})
        entry, nodes = pg, {pg: ("PageNode", "page", {"true": ok, "false": no})}
    else:
        entry, nodes = nid, {nid: ("product-CaptchaNode", "captcha", {"true": ok, "false": no})}
    nodes[ok] = ("ScriptedDecisionNode", "t", {"true": SUCCESS})
    nodes[no] = ("ScriptedDecisionNode", "f", {"true": SUCCESS})
    return fx.tree(tag, entry, nodes)


def drive(tag, tree, tokens):
    r = authenticate(tree)
    view(f"{tag} first", r)
    for token in tokens:
        if r[0] != 200 or "callbacks" not in r[1]:
            return
        body = json.loads(json.dumps(r[1]))
        for c in body["callbacks"]:
            if c["type"] == "ReCaptchaCallback":
                c["input"][0]["value"] = token
            elif c["type"] == "NameCallback":
                c["input"][0]["value"] = "kdemo-g3-user"
        view(f"{tag} token={token!r}", authenticate(tree, body))


try:
    for tag, v3, page in (("v3", True, False), ("v2", False, False), ("v3page", True, True)):
        tree = tree_for(tag, cfg(v3), page)
        drive(tag, tree, [""])
        tree2 = tree_for(tag + "b", cfg(v3), page)
        drive(tag + "b", tree2, ["kdemo-fake-token"])
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        if "delete=404" not in line:
            log("   ", line)
PY
