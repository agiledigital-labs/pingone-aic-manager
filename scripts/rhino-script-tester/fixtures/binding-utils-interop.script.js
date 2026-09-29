// Probe: crypto interop with values made outside AIC, and the edges of what
// each utils method accepts. Safe to delete.
// EC_PUB is a throwaway P-256 public key (X.509 SubjectPublicKeyInfo DER);
// SIG_* are OpenSSL signatures of "interop" under its private key, which was
// never committed. A verify that returns true fixes the key encoding and the
// signature format AIC expects. Generated keys are recorded by DER header and
// length, which are fixed for a key type; the key bytes themselves are random.
function keyed(feature, records) {
  var out = { ok: true, feature: feature };
  for (var i = 0; i < records.length; i++) {
    var rec = records[i];
    var fields = {};
    for (var k in rec) {
      if (k !== "name") {
        fields[k] = rec[k];
      }
    }
    out[rec.name] = fields;
  }
  return JSON.stringify(out).replace(/@[0-9a-f]{4,8}\b/g, "@<hash>");
}
function emit(p) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", p);
  }
  outcome = "ok";
}
function probe(n, f) {
  try {
    return { name: n, ok: true, value: f() };
  } catch (e) {
    return { name: n, ok: false, error: String(e) };
  }
}
function describe(v) {
  var r = { type: typeof v, string: String(v) };
  try {
    r.array = Array.isArray(v);
  } catch (e) {}
  try {
    r.keys = Object.keys(v).sort();
  } catch (e) {}
  try {
    r.length = v.length;
  } catch (e) {}
  return r;
}
function bytes(s) {
  return utils.types.stringToBytes(s);
}
function hex(a) {
  var h = "";
  for (var j = 0; j < a.length; j++) {
    var x = (Number(a[j]) + 256) % 256;
    h += (x < 16 ? "0" : "") + x.toString(16);
  }
  return h;
}
function b64(s) {
  return utils.base64.decodeToBytes(s);
}
var sub = utils.crypto.subtle;
var EC_PUB = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAECVzVmWjfyFzTcIarPr6R3ZTtyNHAL6Izl+wUOuvGWUfBq44Q14wS/6fUQAzdj17ikqoSj9AV+nwAtmIB2FOQeA==";
var SIG_P1363 = "DIEfbOtwIkA5hVeYfPLMIO/ciPI3VmYVv30kAOelYnOrtqND+4gt9J15qziD0Ym05E/BE3DzuQP6ysxCOneXiw==";
var SIG_DER = "MEUCIAyBH2zrcCJAOYVXmHzyzCDv3IjyN1ZmFb99JADnpWJzAiEAq7ajQ/uILfSdeas4g9GJtORPwRNw87kD+srMQjp3l4s=";
var r = [];
r.push(probe("ecdsa/verify/p1363", function () {
  return sub.verify("ECDSA", b64(EC_PUB), bytes("interop"), b64(SIG_P1363));
}));
r.push(probe("ecdsa/verify/der", function () {
  return sub.verify("ECDSA", b64(EC_PUB), bytes("interop"), b64(SIG_DER));
}));
r.push(probe("ecdsa/verify/p1363/tampered", function () {
  return sub.verify("ECDSA", b64(EC_PUB), bytes("interoq"), b64(SIG_P1363));
}));
function header(k, n) {
  return { length: k.length, head: hex(k).substring(0, n * 2) };
}
r.push(probe("rsa/generated/encoding", function () {
  var k = sub.generateKey("RSA");
  // A PKCS#8 RSA key's DER length varies with its integers, so skip it.
  return { publicKey: header(k.publicKey, 24), privateKey: hex(k.privateKey).substring(8, 44) };
}));
r.push(probe("ecdsa/generated/encoding", function () {
  var k = sub.generateKey("ECDSA");
  return { publicKey: header(k.publicKey, 27), privateKey: header(k.privateKey, 35) };
}));
r.push(probe("digest/js-array", function () {
  return hex(sub.digest("SHA-256", [97, 98, 99]));
}));
r.push(probe("digest/SHA-1", function () {
  return hex(sub.digest("SHA-1", bytes("abc")));
}));
r.push(probe("digest/SHA-512", function () {
  return hex(sub.digest("SHA-512", bytes("abc"))).substring(0, 16);
}));
r.push(probe("digest/MD5", function () {
  return hex(sub.digest("MD5", bytes("abc")));
}));
r.push(probe("digest/object", function () {
  return hex(sub.digest({ name: "SHA-256" }, bytes("abc"))).substring(0, 16);
}));
r.push(probe("aes/js-array-key", function () {
  return hex(sub.encrypt("AES", [48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102], bytes("abc")));
}));
r.push(probe("aes/js-array-data", function () {
  return hex(sub.encrypt("AES", bytes("0123456789abcdef"), [97, 98, 99]));
}));
r.push(probe("aes/key-24", function () {
  return hex(sub.encrypt("AES", bytes("0123456789abcdef01234567"), bytes("abc")));
}));
r.push(probe("aes/key-5", function () {
  return hex(sub.encrypt("AES", bytes("short"), bytes("abc")));
}));
r.push(probe("hmac/SHA-512", function () {
  return hex(sub.sign({ name: "HMAC", hash: "SHA-512" }, bytes("key"), bytes("abc"))).substring(0, 16);
}));
r.push(probe("types/bytesToString/js-array", function () {
  return utils.types.bytesToString([97, 98]);
}));
r.push(probe("types/stringToBytes/number", function () {
  return hex(utils.types.stringToBytes(12));
}));
r.push(probe("base64/encode/number", function () {
  return utils.base64.encode(12);
}));
r.push(probe("crypto/randomUUID/1", function () {
  return typeof utils.crypto.randomUUID(1);
}));
r.push(probe("crypto/getRandomValues/js-object", function () {
  var o = { length: 2 };
  utils.crypto.getRandomValues(o);
  return [typeof o[0], typeof o[1]];
}));
emit(keyed("binding-utils-interop", r));
