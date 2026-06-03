// IDM custom-endpoint bindings. Layered on rhino + common.
//
// An endpoint script runs in response to an HTTP call to
// /openidm/endpoint/<name>, so it gets `request` (the CREST request) and
// `context` (the call chain, including the originating HTTP headers/method).

type IdmRequest = {
  method:
    | "create"
    | "read"
    | "update"
    | "delete"
    | "patch"
    | "query"
    | "action";
  resourcePath: string;
  newResourceId?: string;
  revision?: string;
  patchOperations: any;
  content: any;
  action?: string;
  pagedResults: any;
  pagedResultsCookie?: string;
  pagedResultsOffset?: number;
  pageSize?: number;
  queryFilter?: string;
  queryId?: string;
  additionalParameters: Record<string, string>;
};
declare const request: IdmRequest;

type Context = {
  current: {
    parent: {
      parent: {
        parent: {
          headers: Record<string, string[]>;
          method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
        };
      };
    };
  };
};
declare const context: Context;
