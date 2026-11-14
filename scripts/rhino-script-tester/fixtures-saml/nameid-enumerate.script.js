// SAML2_NAMEID_MAPPER probe. Logs scalar facts about the bindings and ends
// on a fixed string, so a successful flow shows whether the completion value
// becomes the assertion NameID.
//
// Never serialises an object: session and request data stay out of the log.

function zzemuR3SKind(v) {
  if (v === null) { return "null"; }
  return typeof v;
}

function zzemuR3SScalar(label, v) {
  var t = zzemuR3SKind(v);
  if (v === null || v === undefined || t === "string" || t === "number" || t === "boolean") {
    var s = String(v);
    if (s.length > 180) { s = "len=" + s.length; }
    logger.info("zzemuR3S " + label + " t=" + t + " v=" + s);
  } else {
    logger.info("zzemuR3S " + label + " t=" + t);
  }
}

function zzemuR3SProbe() {
  logger.info("zzemuR3S-ran");
  zzemuR3SScalar("typeof.helper", nameIDScriptHelper);
  zzemuR3SScalar("typeof.identity", identity);
  zzemuR3SScalar("typeof.nameIDFormat", nameIDFormat);
  zzemuR3SScalar("typeof.remoteEntityId", remoteEntityId);
  zzemuR3SScalar("typeof.hostedEntityId", hostedEntityId);
  var sessionKind = "undeclared";
  try { sessionKind = zzemuR3SKind(session); } catch (e) { sessionKind = "throw"; }
  logger.info("zzemuR3S typeof.session=" + sessionKind);
  zzemuR3SScalar("nameIDFormat", nameIDFormat);
  zzemuR3SScalar("remoteEntityId", remoteEntityId);
  zzemuR3SScalar("hostedEntityId", hostedEntityId);

  var names = [];
  try {
    for (var k in nameIDScriptHelper) {
      var t;
      try { t = typeof nameIDScriptHelper[k]; } catch (e1) { t = "typeof-throw"; }
      names.push(String(k) + ":" + t);
    }
  } catch (e) {
    logger.error("zzemuR3S enum-throw " + String(e));
  }
  names.sort();
  logger.info("zzemuR3S members " + names.join(" "));

  var i;
  for (i = 0; i < names.length; i++) {
    var pair = names[i];
    var cut = pair.lastIndexOf(":");
    var name = pair.substring(0, cut);
    var typ = pair.substring(cut + 1);
    if (typ !== "function") {
      try { zzemuR3SScalar("const." + name, nameIDScriptHelper[name]); }
      catch (e2) { logger.error("zzemuR3S const " + name + " throw " + String(e2)); }
      continue;
    }
    var fn = nameIDScriptHelper[name];
    var len = -1;
    try { len = fn.length; } catch (e3) { len = -2; }
    logger.info("zzemuR3S fn " + name + " length=" + len);
    var getter = len === 0 && (name.indexOf("get") === 0 || name.indexOf("should") === 0 || name.indexOf("is") === 0);
    if (!getter) { continue; }
    try {
      var ret = fn.call(nameIDScriptHelper);
      zzemuR3SScalar("call." + name, ret);
    } catch (e4) {
      logger.error("zzemuR3S call " + name + " throw " + String(e4));
    }
  }

  try { zzemuR3SScalar("identity.exists", identity.exists()); }
  catch (e5) { logger.error("zzemuR3S identity.exists throw " + String(e5)); }
  try { zzemuR3SScalar("identity.getName", identity.getName()); }
  catch (e6) { logger.error("zzemuR3S identity.getName throw " + String(e6)); }
  try {
    var mails = identity.getAttributeValues("mail");
    logger.info("zzemuR3S mail.typeof=" + zzemuR3SKind(mails));
    var n = -1;
    try { n = mails.length; } catch (e7) {
      try { n = mails.size(); } catch (e8) { n = -2; }
    }
    logger.info("zzemuR3S mail.len=" + n);
    if (n > 0) {
      var first;
      try { first = mails[0]; } catch (e9) { first = mails.get(0); }
      zzemuR3SScalar("mail0", first);
    }
  } catch (e10) {
    logger.error("zzemuR3S mail throw " + String(e10));
  }

  return "zzemuR3S-nameid-return";
}

zzemuR3SProbe();
