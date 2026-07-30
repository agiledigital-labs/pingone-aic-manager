// Probe: does `for (var x of iterable)` work at all on AM? Safe to delete.
//
// The existing const-in-for-of fixture only proves that `const` in the for-of
// HEAD is a parse error — it says nothing about the loop form itself. This
// matters for the workspace tsconfig: `for-of` type-checks only when the `lib`
// list declares Symbol.iterator, and AM has no `Symbol` at runtime
// (2026-07-30), so if for-of also fails at runtime then narrowing the lib to
// reject it is correct rather than a false positive.
//
// Probed over both an array and a string, with `var` in the head so the known
// `const` parse error cannot mask the result.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

function probe(name, fn) {
  try {
    return { name: name, ok: true, value: String(fn()) };
  } catch (e) {
    return { name: name, ok: false, error: String(e) };
  }
}

try {
  var results = [];
  results.push(
    probe("for-of over array", function () {
      var sum = 0;
      for (var n of [1, 2, 3]) {
        sum += n;
      }
      return sum;
    })
  );
  results.push(
    probe("for-of over string", function () {
      var out = "";
      for (var ch of "ab") {
        out += ch;
      }
      return out;
    })
  );
  // Control: the equivalent index loop, which is the documented alternative.
  results.push(
    probe("index loop control", function () {
      var arr = [1, 2, 3];
      var sum = 0;
      for (var i = 0; i < arr.length; i++) {
        sum += arr[i];
      }
      return sum;
    })
  );
  emit({ ok: true, feature: "for-of-var", value: results });
} catch (e) {
  emit({ ok: false, feature: "for-of-var", error: String(e) });
}
