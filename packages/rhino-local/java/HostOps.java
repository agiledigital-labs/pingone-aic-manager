import java.nio.charset.StandardCharsets;
import java.security.KeyFactory;
import java.security.KeyPair;
import java.security.KeyPairGenerator;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.security.Signature;
import java.security.spec.ECGenParameterSpec;
import java.security.spec.PKCS8EncodedKeySpec;
import java.security.spec.X509EncodedKeySpec;
import java.util.Base64;
import java.util.UUID;
import javax.crypto.Cipher;
import javax.crypto.Mac;
import javax.crypto.SecretKeyFactory;
import javax.crypto.spec.PBEKeySpec;
import javax.crypto.spec.SecretKeySpec;
import org.mozilla.javascript.BaseFunction;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.JavaScriptException;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Wrapper;

/** Narrow Java bridge captured by trusted mocks, then removed before user code runs. */
public final class HostOps extends BaseFunction {
  private static final SecureRandom RANDOM = new SecureRandom();

  // A real Java byte[], as AIC returns: the same NativeJavaArray a script gets
  // from java.lang.String#getBytes, which the shutter already allows.
  private static Object wrapBytes(Context cx, Scriptable scope, byte[] bytes) {
    return cx.getWrapFactory().wrap(cx, scope, bytes, byte[].class);
  }

  private static byte[] toBytes(Object value) {
    return (byte[]) Context.jsToJava(value, byte[].class);
  }

  @Override
  public Object call(Context cx, Scriptable scope, Scriptable thisObj, Object[] args) {
    try {
      return dispatch(cx, scope, args);
    } catch (RuntimeException e) {
      throw e;
    } catch (Exception e) {
      throw Context.reportRuntimeError(e.toString());
    }
  }

  private Object dispatch(Context cx, Scriptable scope, Object[] args) throws Exception {
    String op = (String) Context.jsToJava(args[0], String.class);
    switch (op) {
      case "base64Encode": {
        byte[] b = toBytes(args[1]);
        boolean url = Context.toBoolean(args[2]);
        return url ? Base64.getUrlEncoder().withoutPadding().encodeToString(b)
            : Base64.getEncoder().encodeToString(b);
      }
      case "base64Decode": {
        String s = (String) Context.jsToJava(args[1], String.class);
        try {
          return wrapBytes(cx, scope, Context.toBoolean(args[2])
              ? Base64.getUrlDecoder().decode(s) : Base64.getDecoder().decode(s));
        } catch (IllegalArgumentException e) {
          // Thrown as a bare JS value so the mock can rethrow it without a
          // location suffix, as AIC reports it.
          throw new JavaScriptException(e.getMessage(), null, 0);
        }
      }
      case "utf8": return wrapBytes(cx, scope,
          ((String) Context.jsToJava(args[1], String.class)).getBytes(StandardCharsets.UTF_8));
      case "fromUtf8": return new String(toBytes(args[1]), StandardCharsets.UTF_8);
      case "uuid": return UUID.randomUUID().toString();
      case "randomInt32": return Integer.valueOf(RANDOM.nextInt());
      case "digest": return wrapBytes(cx, scope, MessageDigest.getInstance((String) Context.jsToJava(args[1], String.class))
          .digest(toBytes(args[2])));
      case "hmac": {
        String alg = (String) Context.jsToJava(args[1], String.class);
        Mac mac = Mac.getInstance("SHA-256".equals(alg) ? "HmacSHA256" : alg);
        mac.init(new SecretKeySpec(toBytes(args[2]), mac.getAlgorithm()));
        return wrapBytes(cx, scope, mac.doFinal(toBytes(args[3])));
      }
      case "aes": {
        Cipher c = Cipher.getInstance("AES/ECB/PKCS5Padding");
        c.init(Context.toBoolean(args[1]) ? Cipher.ENCRYPT_MODE : Cipher.DECRYPT_MODE,
            new SecretKeySpec(toBytes(args[2]), "AES"));
        return wrapBytes(cx, scope, c.doFinal(toBytes(args[3])));
      }
      case "pbkdf2": {
        byte[] password = toBytes(args[1]);
        // AIC casts the salt rather than converting it, so a JS array fails
        // (measured); its message also names the webapp class loader.
        Object rawSalt = args[2] instanceof Wrapper ? ((Wrapper) args[2]).unwrap() : args[2];
        if (!(rawSalt instanceof byte[])) {
          throw new JavaScriptException("class " + rawSalt.getClass().getName() + " cannot be cast to class [B", null, 0);
        }
        byte[] salt = (byte[]) rawSalt;
        int iterations = ((Number) Context.jsToJava(args[3], Number.class)).intValue();
        int bits = ((Number) Context.jsToJava(args[4], Number.class)).intValue();
        String hash = (String) Context.jsToJava(args[5], String.class);
        PBEKeySpec spec = new PBEKeySpec(new String(password, StandardCharsets.UTF_8).toCharArray(), salt, iterations, bits);
        return wrapBytes(cx, scope, SecretKeyFactory.getInstance("PBKDF2WithHmac" + hash.replace("-", ""))
            .generateSecret(spec).getEncoded());
      }
      case "generateSecret": {
        int size = ((Number) Context.jsToJava(args[1], Number.class)).intValue();
        byte[] bytes = new byte[size]; RANDOM.nextBytes(bytes); return wrapBytes(cx, scope, bytes);
      }
      case "generateKeyPair": {
        String alg = (String) Context.jsToJava(args[1], String.class);
        KeyPairGenerator gen = KeyPairGenerator.getInstance("ECDSA".equals(alg) ? "EC" : alg);
        if ("RSA".equals(alg)) gen.initialize(2048);
        if ("ECDSA".equals(alg)) gen.initialize(new ECGenParameterSpec("secp256r1"));
        KeyPair pair = gen.generateKeyPair();
        Scriptable result = cx.newObject(scope);
        ScriptableObject.putProperty(result, "privateKey", wrapBytes(cx, scope, pair.getPrivate().getEncoded()));
        ScriptableObject.putProperty(result, "publicKey", wrapBytes(cx, scope, pair.getPublic().getEncoded()));
        return result;
      }
      case "rsa": {
        boolean encrypt = Context.toBoolean(args[1]);
        byte[] key = toBytes(args[2]);
        byte[] data = toBytes(args[3]);
        // RSA transformation and encoded key formats were not independently measured; use JDK defaults.
        KeyFactory factory = KeyFactory.getInstance("RSA");
        Cipher cipher = Cipher.getInstance("RSA");
        cipher.init(encrypt ? Cipher.ENCRYPT_MODE : Cipher.DECRYPT_MODE,
            encrypt ? factory.generatePublic(new X509EncodedKeySpec(key)) : factory.generatePrivate(new PKCS8EncodedKeySpec(key)));
        return wrapBytes(cx, scope, cipher.doFinal(data));
      }
      case "ecdsaSign": {
        byte[] key = toBytes(args[1]);
        Signature sig = Signature.getInstance("SHA256withECDSAinP1363Format");
        sig.initSign(KeyFactory.getInstance("EC").generatePrivate(new PKCS8EncodedKeySpec(key)), RANDOM);
        sig.update(toBytes(args[2])); return wrapBytes(cx, scope, sig.sign());
      }
      case "ecdsaVerify": {
        byte[] key = toBytes(args[1]);
        Signature sig = Signature.getInstance("SHA256withECDSAinP1363Format");
        sig.initVerify(KeyFactory.getInstance("EC").generatePublic(new X509EncodedKeySpec(key)));
        sig.update(toBytes(args[2]));
        return Boolean.valueOf(sig.verify(toBytes(args[3])));
      }
      default: throw Context.reportRuntimeError("unknown host operation: " + op);
    }
  }
}
