#!/usr/bin/env bash
# experiment-journey-esv-placeholders.sh
#
# Question: how does a node-designer (custom) node's property read an ESV placeholder, bare or
# wrapped as `{"$bool": ...}` / `{"$int": ...}` / `{"$string": ...}`? The instance is PUT with the
# placeholder, and the script reports what `properties.<name>` is at run time (typeof, and whether
# it equals the ESV read through systemEnv). A new ESV needs a tenant restart, so this reads an
# ESV the sandbox already holds of each type (picked by type, never printed) and creates none.
#
# No ESV value leaves the journey script: it compares each property with the ESVs itself and
# reports only type names, booleans and, for the literal cases, a value drawn from a fixed list of
# the probe's own literals (anything else reads "<other>", and a text equal to an ESV "<esv>").
# The callback and transcript carry those.
#
# Objects are kdemo-g3-*, deleted on exit, deletion confirmed by a GET.

set -uo pipefail
export EXPERIMENT_PREFIX="${EXPERIMENT_PREFIX:-kdemo-g3}"
# shellcheck disable=SC1091  # the prelude is checked on its own
. "$(dirname "${BASH_SOURCE[0]}")/experiment-journey-prelude.sh"

python3 - <<'PY' 2>&1 | "$REDACT" --redact
import json, time, uuid
from experiment_journey_lib import BASE, NODES, REALM, RP, Fixture, authenticate, call, log, hidden, SUCCESS

fx = Fixture("esv")

st, body, _ = call("GET", "/environment/variables?_queryFilter=true", api="protocol=1.0,resource=1.0")
variables = body.get("result", []) if isinstance(body, dict) else []


def pick(kind):
    for v in variables:
        n = v["_id"]
        if v.get("expressionType") == kind and not any(w in n for w in ("key", "secret", "password", "token")):
            return n.replace("-", ".")
    return None


E = {k: pick(k) for k in ("bool", "string", "int")}
log("--- ESV types available:", {k: v is not None for k, v in E.items()})

sn = uuid.uuid4().hex
TYPE_PATH = f"/am/json/node-designer/node-type/{sn}-1"
SCRIPT = r'''
var names = ["tb", "ts", "tn"];
// The probe's own literals; a property whose text is none of these is reported as "<other>".
var LITERALS = ["true", "false", "TRUE", "maybe", "abc", "7", "7.0", "1.5", "0", "NaN", "null", "undefined", ""];
var ref = ESVREF;
var esv = {};
for (var k in ref) esv[k] = systemEnv.getProperty(ref[k]);
var out = { ref: {} };
for (var k2 in esv) out.ref[k2] = { typeof: typeof esv[k2], isNull: esv[k2] === null };
for (var i = 0; i < names.length; i++) {
  var v = properties[names[i]];
  var text = String(v);
  var r = { typeof: typeof v, esv: {} }, fromEsv = false;
  for (var k3 in esv) {
    var e = esv[k3];
    r.esv[k3] = { same: v === e, sameText: text === String(e), embedded: text === "a-" + String(e) + "-b" };
    fromEsv = fromEsv || r.esv[k3].sameText || r.esv[k3].embedded;
  }
  // A text that matches an ESV is never echoed, even when it is also one of the literals.
  r.literal = fromEsv ? "<esv>" : LITERALS.indexOf(text) >= 0 ? text : "<other>";
  out[names[i]] = r;
}
nodeState.putShared("probe", JSON.stringify(out));
outcome = "next";
'''
try:
    st, b, _ = call("PUT", TYPE_PATH, {
        "serviceName": sn, "version": 1, "displayName": f"kdemo-g3-esv-{sn[:8]}",
        "description": "journey runtime probe. Safe to delete.", "outcomes": ["next"], "outputs": ["*"],
        "inputs": ["*"], "script": SCRIPT.replace("ESVREF", json.dumps({k: v for k, v in E.items() if v})),
        "errorOutcome": False, "tags": [], "libraries": [],
        "properties": {
            "tb": {"title": "tb", "description": "", "type": "BOOLEAN", "required": False, "multivalued": False},
            "ts": {"title": "ts", "description": "", "type": "STRING", "required": False, "multivalued": False},
            "tn": {"title": "tn", "description": "", "type": "NUMBER", "required": False, "multivalued": False}}})
    log("--- custom type PUT:", st, "" if st in (200, 201) else json.dumps(b)[:300])
    fx.track("nodetype", TYPE_PATH)
    ntype = f"designer-{sn}"
    show = fx.scripted("show", 'if (callbacks.isEmpty()) { callbacksBuilder.hiddenValueCallback("r", String(nodeState.get("probe"))); } else { action.goTo("true"); }', ["true"])

    def case(label, props):
        try:
            nid = fx.node(ntype, props)
        except RuntimeError as e:
            log(f"=== {label}: instance PUT refused: {str(e)[:260]}")
            return
        tree = fx.tree(label, nid, {
            nid: (ntype, label, {"next": show}),
            show: ("ScriptedDecisionNode", "show", {"true": SUCCESS})})
        stt, resp, _ = authenticate(tree)
        if stt == 400 and "No Configuration" in json.dumps(resp):
            log(f"    (retrying {label}: {json.dumps(resp)[:80]})")
            time.sleep(3)
            stt, resp, _ = authenticate(tree)
        r = hidden(resp, "r") if isinstance(resp, dict) else None
        if r is None:
            log(f"=== {label}: authenticate HTTP {stt}: {json.dumps(resp)[:200]}")
            return
        d = json.loads(r)
        log(f"=== {label}: " + json.dumps({k: d[k] for k in ("tb", "ts", "tn")}) + " ref=" + json.dumps(d["ref"]))

    case("bool-plain-true", {"tb": True})
    case("bool-wrapped-literal", {"tb": {"$bool": "true"}})
    case("bool-wrapped-literal-false", {"tb": {"$bool": "false"}})
    case("string-wrapped-literal", {"ts": {"$string": "abc"}})
    case("int-wrapped-literal", {"tn": {"$int": "7"}})
    case("bool-wrapped-not-a-bool", {"tb": {"$bool": "maybe"}})
    case("int-wrapped-not-a-number", {"tn": {"$int": "abc"}})
    case("int-wrapped-decimal", {"tn": {"$int": "1.5"}})
    case("bool-wrapped-capitalised", {"tb": {"$bool": "TRUE"}})
    case("bool-wrapped-unset-esv", {"tb": {"$bool": "&{esv.kdemo.g3.nothing}"}})
    case("string-bare-unset-esv", {"ts": "&{esv.kdemo.g3.nothing}"})
    if E["bool"]:
        case("bool-wrapped-esv", {"tb": {"$bool": "&{%s}" % E["bool"]}})
        case("bool-bare-esv", {"tb": "&{%s}" % E["bool"]})
        case("string-bare-bool-esv", {"ts": "&{%s}" % E["bool"]})
    if E["string"]:
        case("string-bare-esv", {"ts": "&{%s}" % E["string"]})
        case("string-wrapped-esv", {"ts": {"$string": "&{%s}" % E["string"]}})
        case("string-embedded-esv", {"ts": "a-&{%s}-b" % E["string"]})
    if E["int"]:
        case("number-wrapped-int-esv", {"tn": {"$int": "&{%s}" % E["int"]}})
        case("number-bare-int-esv", {"tn": "&{%s}" % E["int"]})
finally:
    log("--- cleanup")
    for line in fx.cleanup():
        log("   ", line)
PY
