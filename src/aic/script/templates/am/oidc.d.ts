/*
 * Class reference from the default OIDC script:
 * (1) UserInfoClaims - https://backstage.forgerock.com/docs/am/7/apidocs/org/forgerock/oauth2/core/UserInfoClaims.html.
 * (2) Claim - https://backstage.forgerock.com/docs/am/7/apidocs/org/forgerock/openidconnect/Claim.html).
 *         An instance of org.forgerock.openidconnect.Claim has methods to access
 *         the claim name, requested values, locale, and whether the claim is essential.
 * (3) AMIdentity - https://backstage.forgerock.com/docs/am/7/apidocs/com/sun/identity/idm/AMIdentity.html.
 * (4) SSOToken - https://backstage.forgerock.com/docs/am/7/apidocs/com/iplanet/sso/SSOToken.html.
 * (5) Map - https://docs.oracle.com/en/java/javase/11/docs/api/java.base/java/util/HashMap.html,
 *           or https://docs.oracle.com/en/java/javase/11/docs/api/java.base/java/util/LinkedHashMap.html.
 * (6) Set - https://docs.oracle.com/en/java/javase/11/docs/api/java.base/java/util/HashSet.html.
 * (7) List - https://docs.oracle.com/en/java/javase/11/docs/api/java.base/java/util/ArrayList.html.
 * (8) Client - https://backstage.forgerock.com/docs/am/7/apidocs/org/forgerock/http/Client.html.
 */
type StringLike = string | JavaString;

interface JavaString {
  new (value: StringLike): JavaString;

  includes(value: StringLike): boolean;

  split(separator: StringLike): JavaArray<JavaString>;
}

interface JavaArray<T = StringLike> {
  [index: number]: T | null | undefined;

  size(): number;

  get(index: number): T | null | undefined;

  contains(value: T): boolean;

  asList(): any[];

  toArray(): T[];

  isEmpty(): boolean;
}

interface JavaMap<Key = JavaString, Value = JavaString> {
  get(key: Key): Value | null;
}

interface JavaSet<T = JavaString> {
  contains(key: T): boolean;
  size(): number;
  toArray(): T[];
}

interface Session {
  getProperty(name: string): JavaArray;
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
  allowedGrantTypes: JavaArray;
  clientId: JavaString;
  allowedScopes: JavaArray;
  allowedResponseTypes: JavaArray;
}

interface Logger {
  error(message: StringLike): void;
  errorEnabled(): boolean;
  message(message: StringLike): void;
  messageEnabled(): boolean;
  warning(message: StringLike): void;
  warningEnabled(): boolean;
}

interface AMIdentity {
  getAttribute(attributeName: string): JavaArray<string>;
  getAttributes(): JavaArray<string>;
  getAttributes(attributeNames: string[]): JavaArray<string>;
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
declare const logger: Logger;
declare const httpClient: object; // TODO: https://backstage.forgerock.com/docs/am/7/apidocs/org/forgerock/http/Client.html
declare const identity: AMIdentity;
declare const JavaImporter: (...classes: JavaClass[]) => void;
declare const session: Session;

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