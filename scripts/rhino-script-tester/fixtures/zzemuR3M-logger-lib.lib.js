// LIBRARY required by the zzemuR3M logger-rebinding probes.
// load() returns an info() that calls the library's own `logger`.
// readCallerOnly() reads nodeState, which a library does not have.
// Safe to delete.
exports.load = function () {
  return {
    info: function (m) {
      logger.info(m);
    }
  };
};

exports.readCallerOnly = function () {
  return String(nodeState);
};
