// rhino-local-corpus
// row: es2015-methods
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | ES2015 methods
// probed: 2026-06-03
// aic-compiled: true
// aic-evaluated: true
// aic-result: true|2|a,b|false,false,false|true|true|true|abab|{"a":1,"b":2}|a,b
// aic-summary: Array includes/find/from/fill, String includes/startsWith/endsWith/repeat, Object assign/keys all work
// verdict: check

function probe(name, fn) {
  try {
    return String(fn());
  } catch (e) {
    return "ERR:" + String(e);
  }
}

var bits = [];
bits.push(
  probe("Array.includes", function () {
    return [1, 2, 3].includes(2);
  })
);
bits.push(
  probe("Array.find", function () {
    return [1, 2, 3].find(function (n) {
      return n > 1;
    });
  })
);
bits.push(
  probe("Array.from", function () {
    return Array.from("ab").join(",");
  })
);
bits.push(
  probe("Array.fill", function () {
    return new Array(3).fill(false).join(",");
  })
);
bits.push(
  probe("String.includes", function () {
    return "hello".includes("ell");
  })
);
bits.push(
  probe("String.startsWith", function () {
    return "hello".startsWith("he");
  })
);
bits.push(
  probe("String.endsWith", function () {
    return "hello".endsWith("lo");
  })
);
bits.push(
  probe("String.repeat", function () {
    return "ab".repeat(2);
  })
);
bits.push(
  probe("Object.assign", function () {
    return JSON.stringify(Object.assign({}, { a: 1 }, { b: 2 }));
  })
);
bits.push(
  probe("Object.keys", function () {
    return Object.keys({ a: 1, b: 2 }).join(",");
  })
);
__result = bits.join("|");
