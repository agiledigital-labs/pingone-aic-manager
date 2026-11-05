// Invented stand-in for rhino-local tests; not ForgeRock code. A module:
// `hidden` must stay module-local, and `object` is the caller's binding.
/* global exports, object */
// eslint-disable-next-line @typescript-eslint/no-unused-vars
var hidden = "module-local";
exports.label = function (prefix) {
  return prefix + ":" + object.text;
};
