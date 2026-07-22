// GENERATED from docs/api/bindings/oauth2-dcr-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OAUTH2_DYNAMIC_CLIENT_REGISTRATION.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.

declare const softwareStatement: object;
declare const requestProperties: object;
declare const operation: StringLike;
interface ClientIdentity {
  getName(): StringLike;
  store(): void;
  setAttribute(attributeName: StringLike, attributeValues: any[]): void;
  addAttribute(attributeName: StringLike, attributeValue: StringLike): void;
  setDisplayName(displayName: any[]): object;
  setScope(allowedGrantScopes: any[]): object;
  getAttributeValues(attributeName: StringLike): any[];
  getUniversalId(): StringLike;
  setClientSecret(clientSecret: StringLike): object;
  setClientType(clientType: StringLike): object;
  setDefaultMaxAge(defaultMaxAge: number): object;
  setDefaultMaxAgeEnabled(enforceDefaultMaxAge: boolean): object;
  setJwksCacheTimeout(jwksCacheTimeout: number): object;
  setJwksCacheMissCacheTime(jwksCacheMissCacheTime: number): object;
  setSectorIdentifierUri(sectorIdentifierUri: StringLike): object;
  setClientName(clientName: any[]): object;
  setClientUri(uris: any[]): object;
  setLogoUri(uris: any[]): object;
  setSubjectType(subjectType: StringLike): object;
  setClientSessionURI(clientSessionURI: StringLike): object;
  setResponseTypes(responseTypes: any[]): object;
  setAuthorizationCodeLifeTime(authorizationCodeLifeTime: number): object;
  setRequestUris(requestUris: any[]): object;
  setPolicyUri(uris: any[]): object;
  setIdTokenEncryptionEnabled(idTokenEncryptionEnabled: boolean): object;
  setTreeName(treeName: StringLike): object;
  setAccessTokenLifeTime(accessTokenLifeTime: number): object;
  setRefreshTokenLifeTime(refreshTokenLifeTime: number): object;
  setJwtTokenLifeTime(jwtTokenLifeTime: number): object;
  setContacts(contacts: any[]): object;
  setClaimsRedirectUris(claimsRedirectUris: any[]): object;
  setDefaultAcrValues(defaultAcrValues: any[]): object;
  setGrantTypes(grantTypes: any[]): object;
  setSoftwareVersion(softwareVersion: StringLike): object;
  setBackChannelLogoutUri(uri: StringLike): object;
  setJwks(jwks: StringLike): object;
  setPublicKeySelector(selector: StringLike): object;
  setJwksUri(jwksUri: StringLike): object;
  setX509(x509: StringLike): object;
  setTlsClientAuthX509Cert(x509Cert: StringLike): object;
  setTlsClientAuthSubjectDn(subjectDn: StringLike): object;
  setTlsCertificateBoundAccessTokens(
    useCertificateBoundAccessTokens: boolean
  ): object;
  setTokenEndpointAuthMethod(tokenEndpointAuthMethod: StringLike): object;
  setIdTokenEncryptedResponseEnc(
    idTokenEncryptedResponseEnc: StringLike
  ): object;
  setUserinfoSignedResponseAlg(userinfoSignedResponseAlg: StringLike): object;
  setUserinfoEncryptedResponseAlg(
    userinfoEncryptedResponseAlg: StringLike
  ): object;
  setUserinfoEncryptedResponseEnc(
    userinfoEncryptedResponseEnc: StringLike
  ): object;
  setUserInfoResponseFormat(userInfoResponseFormat: StringLike): object;
  setAuthorizationResponseSigningAlg(signedResponseAlg: StringLike): object;
  setTosUri(uris: any[]): object;
  setRegistrationAccessToken(accessToken: StringLike): object;
  setDefaultScopes(defaultScopes: any[]): object;
  setClientDescription(displayDescription: any[]): object;
  setTokenEndpointAuthSigningAlg(
    tokenEndpointAuthSigningAlgorithm: StringLike
  ): object;
  setAuthorizationEncryptedResponseAlg(
    encryptedResponseAlg: StringLike
  ): object;
  setAuthorizationEncryptedResponseEnc(
    encryptedResponseMethod: StringLike
  ): object;
  setIdTokenSignedResponseAlg(
    idTokenSignedResponseAlgorithm: StringLike
  ): object;
  setIdTokenEncryptedResponseAlg(
    idTokenEncryptedResponseAlgorithm: StringLike
  ): object;
  setRedirectURIs(redirectURIs: any[]): object;
  setPostLogoutRedirectUris(postLogoutRedirectionURIs: any[]): object;
  setRequestObjectSigningAlg(requestObjectSigningAlg: StringLike): object;
  setRequestObjectEncryptionAlg(requestObjectEncryptedAlg: StringLike): object;
  setRequestObjectEncryptionEnc(
    requestParameterEncryptedEnc: StringLike
  ): object;
  setTokenIntroResponseFormatSelector(
    tokenIntrospectionResponseFormat: StringLike
  ): object;
  setIntrospectionSignedResponseAlg(
    tokenIntrospectionSignedResponseAlg: StringLike
  ): object;
  setIntrospectionEncryptedResponseAlg(
    tokenIntrospectionEncryptedResponseAlg: StringLike
  ): object;
  setIntrospectionEncryptedResponseEnc(
    tokenIntrospectionEncryptedResponseEnc: StringLike
  ): object;
  setSoftwareId(softwareIdentity: StringLike): object;
  isAIAgent(): boolean;
}
declare const clientIdentity: ClientIdentity;
