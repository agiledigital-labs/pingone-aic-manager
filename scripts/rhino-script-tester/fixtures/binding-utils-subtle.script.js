// Probe: utils.crypto.subtle with AM's own algorithm names. Safe to delete.
// binding-utils measured that AM rejects the WebCrypto spellings ("AES-CBC",
// { name: "HMAC", hash: {…} }) and wants a byte[] salt; this retries with the
// names its error messages list: [AES, RSA] and [AES, ECDSA, RSA, HMAC].
function emit(p) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(p));
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
    r.keys = Object.keys(v);
  } catch (e) {}
  try {
    r.length = v.length;
  } catch (e) {}
  return r;
}
function bytes(s) {
  return utils.types.stringToBytes(s);
}
var sub = utils.crypto.subtle;
var r = [];
var names = ["AES", "HMAC", "RSA", "ECDSA"];
for (var i = 0; i < names.length; i++) {
  (function (name) {
    r.push(
      probe("generateKey/string/" + name, function () {
        return describe(sub.generateKey(name));
      })
    );
    r.push(
      probe("generateKey/object/" + name, function () {
        return describe(sub.generateKey({ name: name }));
      })
    );
  })(names[i]);
}
r.push(
  probe("encrypt/decrypt/AES/string", function () {
    var key = bytes("0123456789abcdef");
    var c = sub.encrypt("AES", key, bytes("abc"));
    return {
      ciphertext: describe(c),
      plaintext: utils.types.bytesToString(sub.decrypt("AES", key, c)),
    };
  })
);
r.push(
  probe("encrypt/decrypt/AES/object", function () {
    var key = bytes("0123456789abcdef");
    var c = sub.encrypt({ name: "AES" }, key, bytes("abc"));
    return {
      ciphertext: describe(c),
      plaintext: utils.types.bytesToString(
        sub.decrypt({ name: "AES" }, key, c)
      ),
    };
  })
);
r.push(
  probe("encrypt/AES/generatedKey", function () {
    var k = sub.generateKey("AES");
    return describe(sub.encrypt("AES", k, bytes("abc")));
  })
);
r.push(
  probe("encrypt/RSA/generatedKey", function () {
    var k = sub.generateKey("RSA");
    return describe(sub.encrypt("RSA", k, bytes("abc")));
  })
);
r.push(
  probe("sign/HMAC/object/hash-string", function () {
    return describe(
      sub.sign({ name: "HMAC", hash: "SHA-256" }, bytes("key"), bytes("abc"))
    );
  })
);
r.push(
  probe("sign/HMAC/string/hex", function () {
    var s = sub.sign("HMAC", bytes("key"), bytes("abc"));
    var h = "";
    for (var j = 0; j < s.length; j++) {
      var x = (Number(s[j]) + 256) % 256;
      h += (x < 16 ? "0" : "") + x.toString(16);
    }
    return h;
  })
);
r.push(
  probe("deriveKey/object/PBKDF2", function () {
    return describe(
      sub.deriveKey(
        { name: "PBKDF2", salt: bytes("salt"), iterations: 1, hash: "SHA-256" },
        bytes("password"),
        256
      )
    );
  })
);
r.push(
  probe("deriveKey/string/PBKDF2", function () {
    return describe(sub.deriveKey("PBKDF2", bytes("password"), 256));
  })
);
function hex(a) {
  var h = "";
  for (var j = 0; j < a.length; j++) {
    var x = (Number(a[j]) + 256) % 256;
    h += (x < 16 ? "0" : "") + x.toString(16);
  }
  return h;
}
r.push(
  probe("encrypt/AES/hex-twice", function () {
    var key = bytes("0123456789abcdef");
    return [
      hex(sub.encrypt("AES", key, bytes("abc"))),
      hex(sub.encrypt("AES", key, bytes("abc"))),
    ];
  })
);
r.push(
  probe("deriveKey/object/PBKDF2/hex", function () {
    return hex(
      sub.deriveKey(
        { name: "PBKDF2", salt: bytes("salt"), iterations: 1, hash: "SHA-256" },
        bytes("password"),
        256
      )
    );
  })
);
r.push(
  probe("encrypt/decrypt/RSA/keyPair", function () {
    var k = sub.generateKey("RSA");
    var c = sub.encrypt("RSA", k.publicKey, bytes("abc"));
    return {
      ciphertext: describe(c),
      plaintext: utils.types.bytesToString(sub.decrypt("RSA", k.privateKey, c)),
    };
  })
);
r.push(
  probe("sign/verify/ECDSA/keyPair", function () {
    var k = sub.generateKey("ECDSA");
    var s = sub.sign("ECDSA", k.privateKey, bytes("abc"));
    return {
      signature: describe(s),
      verify: sub.verify("ECDSA", k.publicKey, bytes("abc"), s),
    };
  })
);
emit({ ok: true, feature: "binding-utils-subtle", value: JSON.stringify(r) });
