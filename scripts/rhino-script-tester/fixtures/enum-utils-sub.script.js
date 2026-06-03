// Next-gen probe: enumerate the utils sub-objects (base64/base64url/crypto/types)
// and check wrapped-method arity. Safe to delete; non-destructive.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

function enumerate(o) {
  var out = [];
  for (var k in o) {
    out.push(String(k));
  }
  out.sort();
  return out;
}

try {
  emit({
    ok: true,
    feature: "enum-utils-sub",
    base64: enumerate(utils.base64),
    base64url: enumerate(utils.base64url),
    crypto: enumerate(utils.crypto),
    types: enumerate(utils.types),
    arity: {
      nameCallback: callbacksBuilder.nameCallback.length,
      hiddenValueCallback: callbacksBuilder.hiddenValueCallback.length,
      confirmationCallback: callbacksBuilder.confirmationCallback.length,
      base64Encode: typeof utils.base64.encode,
    },
  });
} catch (e) {
  emit({ ok: false, feature: "enum-utils-sub", error: String(e) });
}
