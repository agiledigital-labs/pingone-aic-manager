/**
 * The persistent cookie `SetPersistentCookieNode` issues and `PersistentCookieDecisionNode` reads.
 *
 * Measured shape (docs/api/25-journey-runtime.md, "Persistent cookie value", 2026-11-25): an HS256
 * JWS (header `typ` JWT, `cty` JWT, `alg` HS256) whose payload is the base64url of a five-part
 * compact JWE (`enc` A128CBC-HS256, `alg` RSA1_5 under the tenant's own key). Signing proves only
 * that the node's `hmacSigningKey` signed it; who the cookie names is inside the encrypted part,
 * which a caller holding the signing key alone cannot produce.
 *
 * What is modelled: the outer JWS exactly (a real HMAC under the configured key), and an inner
 * five-part JWE of the same cipher (A128CBC-HS256) whose key is the emulator's own constant, so a
 * cookie this emulator issued is read back by any emulator, and one signed by hand is not. What is
 * not: the tenant's RSA1_5 key wrap (the `alg` here is `dir`, with the empty key part that implies),
 * the claim names inside the tenant's JWE, which are unreadable, and an expiry rule.
 */
import { createCipheriv, createDecipheriv, createHash, createHmac, timingSafeEqual } from "node:crypto";

const SEAL_KEY = createHash("sha256").update("rhino-local persistent cookie emulation").digest();
const OUTER = { typ: "JWT", cty: "JWT", alg: "HS256" };
const INNER = { typ: "JWT", enc: "A128CBC-HS256", alg: "dir" };

const b64 = (data: Buffer | string): string => Buffer.from(data).toString("base64url");
const unb64 = (text: string): Buffer => Buffer.from(text, "base64url");
const mac = (key: Buffer, ...parts: Buffer[]): Buffer =>
  createHmac("sha256", key).update(Buffer.concat(parts)).digest();

export type CookieClaims = { sub: string | null; iat: number; exp: number };

/** The tag input of A128CBC-HS256: AAD, IV, ciphertext and the AAD's length in bits as 64 bits. */
function tag(aad: Buffer, iv: Buffer, ciphertext: Buffer): Buffer {
  const bits = Buffer.alloc(8);
  bits.writeBigUInt64BE(BigInt(aad.length) * 8n);
  return mac(SEAL_KEY.subarray(0, 16), aad, iv, ciphertext, bits).subarray(0, 16);
}

/** A deterministic IV (derived from the plaintext), so equal inputs give equal cookies under a fixed clock. */
function seal(claims: CookieClaims): string {
  const header = b64(JSON.stringify(INNER));
  const plain = Buffer.from(JSON.stringify(claims));
  const iv = mac(SEAL_KEY, Buffer.from(header), plain).subarray(0, 16);
  const cipher = createCipheriv("aes-128-cbc", SEAL_KEY.subarray(16), iv);
  const ciphertext = Buffer.concat([cipher.update(plain), cipher.final()]);
  return [header, "", b64(iv), b64(ciphertext), b64(tag(Buffer.from(header), iv, ciphertext))].join(".");
}

function open(inner: string): CookieClaims | undefined {
  const [header, , ivText, ctText, tagText] = inner.split(".") as [string, string, string, string, string];
  try {
    if (header !== b64(JSON.stringify(INNER))) return undefined;
    const iv = unb64(ivText);
    const ciphertext = unb64(ctText);
    const expected = tag(Buffer.from(header), iv, ciphertext);
    const given = unb64(tagText);
    if (given.length !== expected.length || !timingSafeEqual(given, expected)) return undefined;
    const decipher = createDecipheriv("aes-128-cbc", SEAL_KEY.subarray(16), iv);
    const plain = JSON.parse(Buffer.concat([decipher.update(ciphertext), decipher.final()]).toString("utf8")) as unknown;
    if (plain === null || typeof plain !== "object" || Array.isArray(plain)) return undefined;
    const { sub, iat, exp } = plain as Record<string, unknown>;
    if ((typeof sub !== "string" && sub !== null) || typeof iat !== "number" || typeof exp !== "number")
      return undefined;
    return { sub, iat, exp };
  } catch {
    return undefined;
  }
}

/** The cookie value for `claims`, signed with the node's base64 `hmacSigningKey`. */
export function mintCookie(claims: CookieClaims, hmacKey: string): string {
  const signing = `${b64(JSON.stringify(OUTER))}.${b64(seal(claims))}`;
  return `${signing}.${createHmac("sha256", Buffer.from(hmacKey, "base64")).update(signing).digest("base64url")}`;
}

/**
 * What the decision node makes of a cookie value, by the cases measured on the tenant:
 * `abort` for a value with no dot (and the empty value) and for a signed payload that is not
 * itself dotted; `false` for two parts, a signature by another key, and an inner part that does
 * not open; `unmeasured` for a shape no probe covered.
 */
export type CookieReading =
  | { kind: "claims"; claims: CookieClaims }
  | { kind: "abort" }
  | { kind: "false" }
  | { kind: "unmeasured"; what: string };

export function readCookie(value: string, hmacKey: string): CookieReading {
  const parts = value.split(".");
  if (parts.length === 1) return { kind: "abort" };
  if (parts.length === 2) return { kind: "false" };
  if (parts.length !== 3) return { kind: "unmeasured", what: `a cookie of ${parts.length} dotted parts` };
  const [head = "", payload = "", signature = ""] = parts;
  const expected = createHmac("sha256", Buffer.from(hmacKey, "base64")).update(`${head}.${payload}`).digest();
  const given = unb64(signature);
  if (given.length !== expected.length || !timingSafeEqual(given, expected)) return { kind: "false" };
  let header: unknown;
  try {
    header = JSON.parse(unb64(head).toString("utf8"));
  } catch {
    return { kind: "unmeasured", what: "a correctly signed cookie whose header is not JSON" };
  }
  if ((header as { alg?: unknown } | null)?.alg !== OUTER.alg)
    return { kind: "unmeasured", what: "a correctly signed cookie whose alg is not HS256" };
  const inner = unb64(payload).toString("utf8");
  const dots = inner.split(".").length - 1;
  if (dots === 0) return { kind: "abort" };
  if (dots !== 4) return { kind: "unmeasured", what: `an inner token of ${dots + 1} dotted parts` };
  const claims = open(inner);
  return claims ? { kind: "claims", claims } : { kind: "false" };
}
