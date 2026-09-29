// Probe: utility encoding, types, random crypto and WebCrypto bindings. Safe to delete.
// Writes no tenant state.
// One payload key per probe, so the corpus can record a gap per probe; and
// Java identity hashes (`[B@6d719625`) masked, because they differ every run.
function keyed(feature, records) {
  var out = { ok: true, feature: feature };
  for (var i = 0; i < records.length; i++) {
    var rec = records[i];
    out[rec.name] = rec.ok
      ? { ok: true, value: rec.value }
      : { ok: false, error: rec.error };
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
  try {
    r.size = v.size();
  } catch (e) {}
  return r;
}
function hex(a) {
  var s = "";
  for (var i = 0; i < a.length; i++) {
    var x = (Number(a[i]) + 256) % 256;
    s += (x < 16 ? "0" : "") + x.toString(16);
  }
  return s;
}
var BCRYPT_HASH =
  "$2b$05$od0xRayrR7/RA8binFq9DOjm7A3KksNwMVJHznMANahWxcFXwtsO2";
var r = [];
var texts = ["hello world", "héllo ✓", "\u00fb\u00ff"];
for (var j = 0; j < texts.length; j++) {
  (function (t, k) {
    r.push(
      probe("base64/string/" + k, function () {
        return {
          encode: describe(utils.base64.encode(t)),
          decode: describe(utils.base64.decode(utils.base64.encode(t))),
          btoa: describe(utils.base64.btoa(t)),
          atob: describe(utils.base64.atob(utils.base64.btoa(t))),
        };
      })
    );
    r.push(
      probe("base64url/string/" + k, function () {
        return {
          encode: describe(utils.base64url.encode(t)),
          decode: describe(utils.base64url.decode(utils.base64url.encode(t))),
          btoa: describe(utils.base64url.btoa(t)),
          atob: describe(utils.base64url.atob(utils.base64url.btoa(t))),
        };
      })
    );
  })(texts[j], j);
}
r.push(
  probe("base64/encode/bytes", function () {
    return utils.base64.encode([251, 255]);
  })
);
r.push(
  probe("base64url/encode/bytes", function () {
    return utils.base64url.encode([251, 255]);
  })
);
r.push(
  probe("base64/decodeToBytes", function () {
    return describe(utils.base64.decodeToBytes("+/8="));
  })
);
r.push(
  probe("base64url/decodeToBytes", function () {
    return describe(utils.base64url.decodeToBytes("-_8="));
  })
);
r.push(
  probe("base64/invalid", function () {
    return utils.base64.decode("%%%bad");
  })
);
r.push(
  probe("base64url/invalid", function () {
    return utils.base64url.decode("%%%bad");
  })
);
r.push(
  probe("types/roundtrip", function () {
    var b = utils.types.stringToBytes("héllo ✓");
    return {
      bytes: describe(b),
      first: [b[0], b[1], b[2]].map(function (x) {
        return { value: x, type: typeof x };
      }),
      roundtrip: utils.types.bytesToString(b),
    };
  })
);
r.push(
  probe("crypto/randomUUID", function () {
    var x = utils.crypto.randomUUID();
    return {
      type: typeof x,
      length: String(x).length,
      uuid: /^[0-9a-f-]{36}$/i.test(String(x)),
    };
  })
);
r.push(
  probe("crypto/getRandomValues", function () {
    var a = [0, 0, 0, 0];
    var x = utils.crypto.getRandomValues(a);
    // Random: record the shape only.
    var types = [];
    for (var j = 0; j < a.length; j++) types.push(typeof a[j]);
    return {
      length: a.length,
      types: types,
      same: x === a,
      array: Array.isArray(x),
    };
  })
);
r.push(
  probe("crypto/checkBcrypt/right", function () {
    if (!BCRYPT_HASH) return "BCRYPT_HASH placeholder";
    return utils.crypto.checkBcrypt(BCRYPT_HASH, "probe-password");
  })
);
r.push(
  probe("crypto/checkBcrypt/wrong", function () {
    if (!BCRYPT_HASH) return "BCRYPT_HASH placeholder";
    return utils.crypto.checkBcrypt(BCRYPT_HASH, "wrong-password");
  })
);
var key = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
r.push(
  probe("subtle/digest/SHA-256", function () {
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var x = sub.digest("SHA-256", msg);
    return { shape: describe(x), hex: hex(x) };
  })
);
r.push(
  probe("subtle/sign/HMAC/options", function () {
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var s = sub.sign({ name: "HMAC", hash: { name: "SHA-256" } }, key, msg);
    return { shape: describe(s), hex: hex(s) };
  })
);
r.push(
  probe("subtle/sign/HMAC/string", function () {
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    return describe(sub.sign("HMAC", key, msg));
  })
);
r.push(
  probe("subtle/verify/right", function () {
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var s = sub.sign("HMAC", key, msg);
    return sub.verify("HMAC", key, msg, s);
  })
);
r.push(
  probe("subtle/verify/tampered", function () {
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var s = sub.sign("HMAC", key, msg);
    return sub.verify("HMAC", key, utils.types.stringToBytes("abd"), s);
  })
);
r.push(
  probe("subtle/encrypt/string", function () {
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    return describe(
      sub.encrypt(
        "AES-CBC",
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        msg
      )
    );
  })
);
r.push(
  probe("subtle/decrypt/string", function () {
    var sub = utils.crypto.subtle;
    return describe(
      sub.decrypt(
        "AES-CBC",
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [1, 2, 3]
      )
    );
  })
);
r.push(
  probe("subtle/encrypt/decrypt/options", function () {
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var sub = utils.crypto.subtle;
    var msg = utils.types.stringToBytes("abc");
    var k = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
      iv = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
      a = { name: "AES-CBC", iv: iv },
      c = sub.encrypt(a, k, msg),
      p = sub.decrypt(a, k, c);
    return { ciphertext: describe(c), plaintext: utils.types.bytesToString(p) };
  })
);
r.push(
  probe("subtle/generateKey/object", function () {
    var sub = utils.crypto.subtle;
    return describe(sub.generateKey({ name: "AES-CBC", length: 128 }));
  })
);
r.push(
  probe("subtle/generateKey/string", function () {
    var sub = utils.crypto.subtle;
    return describe(sub.generateKey("AES-CBC"));
  })
);
r.push(
  probe("subtle/deriveKey/object", function () {
    var sub = utils.crypto.subtle;
    return describe(
      sub.deriveKey(
        { name: "PBKDF2", salt: [1, 2, 3], iterations: 10, hash: "SHA-256" },
        key,
        128
      )
    );
  })
);
r.push(
  probe("subtle/deriveKey/string", function () {
    var sub = utils.crypto.subtle;
    return describe(sub.deriveKey("PBKDF2", key, 128));
  })
);
emit(keyed("binding-utils", r));
