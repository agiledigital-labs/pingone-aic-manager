// Library body for the top-level-const probe. Uploaded as a LIBRARY script and
// required by lib-const-consumer.script.js. If Rhino evaluates a library's
// top-level `const` correctly, `fromConst` round-trips; if it's the same bug as
// decision-node global scope, `fromConst` comes back undefined. Safe to delete.
const TOP_CONST = "lib-const-ok";
var TOP_VAR = "lib-var-ok";
exports.fromConst = TOP_CONST;
exports.fromVar = TOP_VAR;
