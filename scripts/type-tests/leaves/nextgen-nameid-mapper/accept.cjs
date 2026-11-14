// Next-gen NameID mapper (measured 2026-11-14). The assertion NameID is the
// script's completion value; these are the members the SSO actually called.
var nameId = nameIDScriptHelper.getNameIDValue();
if (nameIDScriptHelper.getNameIDFromSession() === null && nameIDScriptHelper.shouldPersistNameIDFormat() === false) {
  logger.info("nameid {}", nameId);
}
if (identity.exists()) {
  var mail = identity.getAttributeValues("mail");
  logger.info("subject {} mail {}", String(identity.getName()), String(mail.length));
}
logger.info("format {} sp {} idp {}", String(nameIDFormat), String(remoteEntityId), String(hostedEntityId));
logger.info("session {}", typeof session);
