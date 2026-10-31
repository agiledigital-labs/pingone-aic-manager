// LIBRARY-context probe for the ES2017 Object members. Uploaded to the sandbox
// as `rhino-lib-es2017-object-probe` with a minted id (`aic script create`),
// and consumed by lib-es2017-object-consumer.script.js. Safe to delete.
function probe(name, fn) {
  try {
    return { name: name, ok: true, value: String(fn()) };
  } catch (e) {
    return { name: name, ok: false, error: String(e) };
  }
}

var results = [];
  results.push(
    probe("typeof Object.keys (control)", function () {
      return typeof Object.keys;
    })
  );
  results.push(
    probe("Object.keys", function () {
      return Object.keys({ a: 1, b: 2 }).join(",");
    })
  );
  results.push(
    probe("typeof Object.values", function () {
      return typeof Object.values;
    })
  );
  results.push(
    probe("Object.values", function () {
      return JSON.stringify(Object.values({ a: 1, b: "x" }));
    })
  );
  results.push(
    probe("typeof Object.entries", function () {
      return typeof Object.entries;
    })
  );
  results.push(
    probe("Object.entries", function () {
      return JSON.stringify(Object.entries({ a: 1, b: "x" }));
    })
  );
  results.push(
    probe("typeof Object.getOwnPropertyDescriptors", function () {
      return typeof Object.getOwnPropertyDescriptors;
    })
  );
exports.results = results;
