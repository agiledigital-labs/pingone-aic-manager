// OIDC claims script bindings.
//
// This models the LEGACY OIDC claims context (Java-heavy: claims as Java maps,
// AMIdentity, JavaImporter, an error/message/warning logger). Next-generation
// OIDC claims is a separate declaration set, `oidc-claims-ng.d.ts`, generated
// from its binding metadata and corrected by what a script measured there
// (docs/api/12-script-bindings-matrix.md, "OIDC claims on both engines",
// 2026-11-10). This leaf includes rhino-1.7.14.d.ts (for the Java interop
// types) plus this file ONLY — it intentionally does NOT pull common.d.ts,
// because the legacy OIDC `logger`/binding shapes differ from the next-gen
// common set.
//
// What AM does with the script: the result is its COMPLETION VALUE (the last
// expression), not a `return`. `new UserInfoClaims(values, compositeScopes)`
// (`values` a Java map, `compositeScopes` a Java map or `null`) is accepted and
// its claims emitted. A plain `{ values, compositeScopes }` object is a 500
// here, though next-gen accepts it, and so is a bare `java.util.LinkedHashMap`;
// `undefined` gives no script claims. `compositeScopes` filters nothing. The
// constructor is declared as an opaque `JavaClass`, so none of that is checked.
//
// Class references from the default OIDC script:
//   UserInfoClaims, Claim (org.forgerock.openidconnect.Claim), AMIdentity,
//   SSOToken, java.util Map/Set/List, org.forgerock.http.Client.

interface Session {
  getProperty(name: StringLike): JavaArray;
}

interface Claim {
  name: JavaString;
  getName(): JavaString;
  values: JavaArray;
  getValues(): JavaArray;
  essential: boolean;
  isEssential(): boolean;
  locale?: JavaString;
  nameWithLocale: JavaString;
  javaLocale?: JavaString;
}

interface RequestProperties {
  requestHeaders: JavaMap<JavaString, JavaArray<JavaString>>;
  requestParams: JavaMap<JavaString, JavaArray<JavaString>>;
  realm: JavaString;
  requestUri: JavaString;
}

interface ClientProperties {
  allowedGrantTypes: JavaArray<JavaString>;
  clientId: JavaString;
  allowedScopes: JavaArray<JavaString>;
  allowedResponseTypes: JavaArray<JavaString>;
  // Measured only empty (2026-11-10): the entry's value type is not claimed.
  customProperties: JavaMap<StringLike, unknown>;
}

// Legacy OIDC logger shape: the classic Debug method names, distinct from the
// next-gen slf4j one. The FORMATTING is slf4j's either way, so the methods take
// `LogFunction` (rhino-1.7.14.d.ts) and get the `{}` arity check — verified on
// the legacy engine 2026-08-27 via the scripted-decision node, which shares this
// engine's Debug logger.
interface OidcLogger {
  // Present (measured 2026-11-10); what it returns was not read in this context.
  // The other legacy contexts hand back a Java string.
  getName(): JavaString;
  error: LogFunction;
  errorEnabled(): boolean;
  message: LogFunction;
  messageEnabled(): boolean;
  warning: LogFunction;
  warningEnabled(): boolean;
}
declare const logger: OidcLogger;

// Parameters are `StringLike`, returns are `JavaString`: a legacy script reaches
// these with JS string literals and with values it pulled out of another Java
// collection, and only the argument side can be widened without lying about
// what comes back. `String(...)` before comparing a result is the safe idiom.
interface AMIdentity {
  /** A Java string (measured 2026-11-10). */
  getName(): JavaString;
  /** A Java string (measured 2026-11-10). */
  getUniversalId(): JavaString;
  /**
   * `true` for the profile identity (measured 2026-11-10). `exists()` is not
   * here: it is absent on this engine.
   */
  isExists(): boolean;
  /**
   * A **`java.util.HashSet`**: `size()`, `toArray()`, `contains()`,
   * `iterator()` — but no `.length`, no `[0]`, no `.get(0)`, no `.includes()`.
   * `String(v.toArray()[0])` is how you read a single-valued attribute.
   *
   * Measured 2026-09-10 in the legacy access-token-modification context, and
   * again through an OIDC flow on 2026-11-10 (`getAttribute("mail")` is a
   * one-element set with no `length`) — see `docs/api/12-script-bindings-matrix.md`.
   *
   * Empty set, never `null`, for an absent attribute AND for a name that does
   * not exist.
   */
  getAttribute(attributeName: StringLike): JavaSet<JavaString>;
  /**
   * A Java `Map` of every attribute, values `HashSet`s — NOT a list, which is
   * what this returned until 2026-09-10. `keySet()` is class-shuttered, so the
   * names cannot be enumerated from here; `get`/`containsKey`/`size` work.
   */
  getAttributes(): JavaMap<JavaString, JavaSet<JavaString>>;
  /**
   * The `Set`-taking overload. Java's parameter is a `java.util.Set`, and Rhino
   * does not convert a JS array to one — the same trap as `setScope` in the
   * token-modification context, where a JS array throws (verified 2026-08-27).
   * Build it with `new java.util.HashSet()`.
   */
  getAttributes(attributeNames: JavaSet<JavaString>): JavaMap<JavaString, JavaSet<JavaString>>;
}

interface JavaClass {}

declare const scopes: JavaSet<JavaString>;
declare const claims: JavaMap<JavaString, Object>;
declare const requestedClaims: JavaMap<JavaString, JavaSet<JavaString>>;
declare const claimObjects: JavaArray<Claim>;
declare const requestedTypedClaims: JavaArray<Claim>;
declare const claimsLocales: JavaArray<JavaString>;
declare const requestProperties: RequestProperties;
declare const clientProperties: ClientProperties;
declare const scriptName: JavaString;
declare const identity: AMIdentity;
// Bound, and `null` on a password grant (measured 2026-11-10): guard it.
declare const session: Session | null;
declare const httpClient: object; // org.forgerock.http.Client (legacy) — shape TBD
declare const JavaImporter: (...classes: JavaClass[]) => void;

declare const org: {
  forgerock: {
    oauth2: {
      core: {
        exceptions: {
          InvalidRequestException: JavaClass;
        };
        UserInfoClaims: JavaClass;
      };
    };
    openidconnect: {
      Claim: JavaClass;
    };
  };
};
declare const java: {
  util: {
    LinkedHashMap: JavaClass;
    ArrayList: JavaClass;
  };
};
