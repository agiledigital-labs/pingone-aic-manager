// Common IDM bindings shared by all IDM script families.
// Layered on top of rhino-1.7.14.d.ts.
//
// IDM binding availability is inferred from the prior template + docs; not yet
// runtime-probed (no local IDM sample corpus). See docs/api/12 IDM open items.

type LogFunction = (message: StringLike, ...args: any[]) => void;
interface Logger {
  trace: LogFunction;
  debug: LogFunction;
  info: LogFunction;
  warn: LogFunction;
  error: LogFunction;
}
declare const logger: Logger;

declare const console: {
  log(message?: any, ...args: any[]): void;
};

type Patch =
  | {
      operation: "add" | "replace";
      field: string;
      value: string;
    }
  | {
      operation: "remove";
      field: string;
    };

// The CREST call chain, shared by endpoint + schedule scripts. The originating
// HTTP request is at `context.http` for endpoint calls (verified 2026-06-04);
// scheduled runs have no HTTP request, so `http` may be absent there. Other
// contexts (security, oauth2, transactionId, …) vary, hence the index signature.
interface IdmContext {
  http?: {
    method: string;
    path: string;
    headers: Record<string, string>;
    parameters: Record<string, string>;
  };
  security?: {
    authenticationId: string;
    authorization: { id: string; component: string; roles: string[] };
  };
  [key: string]: any;
}
declare const context: IdmContext;

declare const identityServer: {
  getProperty(name: string, defaultValue?: string, substitute?: boolean): string;
  getInstallLocation(): string;
  getProjectLocation(): string;
  getWorkingLocation(): string;
};

interface OpenIdm {
  read(
    path: string,
    params?: Record<string, string> | null,
    fields?: string[]
  ): any;
  query(path: string, params: { _queryFilter: string }): any;
  create(
    path: string,
    newResourceId: string | null,
    content: Record<string, any> | null,
    params?: Record<string, string> | null
  ): any;
  update(
    path: string,
    revision: string | null,
    content: Record<string, any> | null,
    params?: Record<string, string> | null
  ): any;
  patch(path: string, revision: string | null, patch: Patch[]): any;
  delete(
    path: string,
    revision: string | null,
    params?: Record<string, string> | null
  ): any;
  action(
    path: string,
    actionName: string,
    content?: Record<string, any> | null,
    params?: Record<string, string> | null
  ): any;
}
declare const openidm: OpenIdm;
