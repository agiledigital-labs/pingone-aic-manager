// Probe: ES2015 Array/String/Object prototype methods real scripts rely on.
// Safe to delete. Each method is probed independently so one missing method
// does not mask the others.
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
    probe("Array.includes", function () {
      return [1, 2, 3].includes(2);
    })
  );
  results.push(
    probe("Array.find", function () {
      return [1, 2, 3].find(function (n) {
        return n > 1;
      });
    })
  );
  results.push(
    probe("Array.from", function () {
      return Array.from("ab").join(",");
    })
  );
  results.push(
    probe("String.includes", function () {
      return "hello".includes("ell");
    })
  );
  results.push(
    probe("String.startsWith", function () {
      return "hello".startsWith("he");
    })
  );
  results.push(
    probe("String.endsWith", function () {
      return "hello".endsWith("lo");
    })
  );
  results.push(
    probe("String.repeat", function () {
      return "ab".repeat(2);
    })
  );
  results.push(
    probe("Object.assign", function () {
      return JSON.stringify(Object.assign({}, { a: 1 }, { b: 2 }));
    })
  );
  results.push(
    probe("Object.keys", function () {
      return Object.keys({ a: 1, b: 2 }).join(",");
    })
  );
  emit({ ok: true, feature: "es2015-methods", value: results });
} catch (e) {
  emit({ ok: false, feature: "es2015-methods", error: String(e) });
}
