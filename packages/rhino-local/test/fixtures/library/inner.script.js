exports.stamp = function () {
  var name = "err";
  var loggerName = "err";
  var node = "err";
  try { name = String(scriptName); } catch (e) { name = "err"; }
  try { loggerName = String(logger.getName()); } catch (e) { loggerName = "err"; }
  try { node = typeof nodeState; } catch (e) { node = "throw"; }
  return { tag: "inner", scriptName: name, loggerName: loggerName, nodeState: node };
};
