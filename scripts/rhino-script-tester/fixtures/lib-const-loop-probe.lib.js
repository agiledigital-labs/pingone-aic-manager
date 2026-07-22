// Library body for the loop-body-const-in-function probe. Uploaded as a
// LIBRARY script and required by lib-const-loop-consumer.script.js. Probes the
// exact shape library code uses in practice: a function containing a loop with
// a `const` declared per iteration. A correct run exports "0,2,4"; the silent
// Rhino bug would yield ",," (each read undefined). Safe to delete.
function doubleAll() {
  var out = [];
  for (var i = 0; i < 3; i++) {
    const doubled = i * 2;
    out.push(doubled);
  }
  return out.join(",");
}
exports.fromLoopConst = doubleAll();
