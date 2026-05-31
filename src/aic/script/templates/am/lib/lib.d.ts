// Server-side script types (Policy, OIDC Claims, etc.)

// Policy script context
export interface PolicyContext {
  request: PolicyRequest;
  response: PolicyResponse;
  identity: Map<string, any>;
  environment: Map<string, any>;
  session: Map<string, any>;
  authorization: Authorization;
}

export interface PolicyRequest {
  uri: string;
  method: string;
  headers: Map<string, string[]>;
  cookies: Map<string, string>;
  parameters: Map<string, string[]>;
  entity: string;
}

export interface PolicyResponse {
  status: number;
  headers: Map<string, string[]>;
  entity: string;
}

export interface Authorization {
  getSubject(): Subject;
  getResource(): string;
  getAction(): string;
  getEnvironment(): Map<string, any>;
}

export interface Subject {
  getPrincipal(): string;
  getPrivateCredentials(): Set<any>;
  getPublicCredentials(): Set<any>;
}

// OIDC Claims script context
export interface ClaimsContext {
  claims: Map<string, any>;
  session: Map<string, any>;
  identity: Map<string, any>;
  scopes: Set<string>;
  requestProperties: Map<string, any>;
}

export {};