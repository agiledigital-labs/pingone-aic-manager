// IDM custom-endpoint bindings. Layered on rhino + common.
//
// `request` shape verified per CREST method 2026-06-04 by echoing the binding
// from a throwaway endpoint (see docs/api/11-idm-endpoints.md). Typed as a
// discriminated union on `method` so e.g. `if (request.method === "create")`
// narrows to the create fields.

/** Common to every request method. `fields` is the `_fields` selector list. */
interface IdmRequestBase {
  resourcePath: string;
  additionalParameters: Record<string, string>;
  fields: string[];
}

/** A single patch operation (request.patchOperations is an array of these). */
interface PatchOperation {
  operation: "add" | "remove" | "replace" | "increment" | "move" | "copy" | "transform";
  field: string;
  value?: any;
  from?: string;
}

type IdmRequest =
  | (IdmRequestBase & { method: "read" })
  | (IdmRequestBase & { method: "create"; newResourceId: string | null; content: any })
  | (IdmRequestBase & { method: "update"; revision: string | null; content: any })
  | (IdmRequestBase & {
      method: "patch";
      revision: string | null;
      patchOperations: PatchOperation[];
    })
  | (IdmRequestBase & { method: "delete"; revision: string | null })
  | (IdmRequestBase & { method: "action"; action: string; content: any })
  | (IdmRequestBase & {
      method: "query";
      queryFilter: string | null;
      queryId: string | null;
      queryExpression: string | null;
      pageSize: number;
      pagedResultsOffset: number;
      pagedResultsCookie: string | null;
      sortKeys: string[];
    });

declare const request: IdmRequest;

// The CREST call chain. The originating HTTP request is at `context.http`
// (verified keys below); other contexts (security, oauth2, transactionId, …)
// are present but vary, hence the index signature.
interface IdmContext {
  http: {
    method: string;
    path: string;
    headers: Record<string, string>;
    parameters: Record<string, string>;
  };
  security: {
    authenticationId: string;
    authorization: { id: string; component: string; roles: string[] };
  };
  [key: string]: any;
}
declare const context: IdmContext;

// ---- response shapes -----------------------------------------------------
//
// An endpoint script RETURNS its response (the last evaluated expression). TS
// can't type-check a script's return value, so these are documentation aliases
// you can annotate with `/** @type {IdmQueryResult} */` for editor help.
//
// IMPORTANT: a `query` handler MUST return an IdmQueryResult — returning a plain
// object fails at runtime ("Script returned unexpected query result structure",
// verified 2026-06-04). read/create/update/patch/delete return a resource object.

/** A resource object (read/create/update/patch return one). */
type IdmResource = { _id?: string; _rev?: string; [key: string]: any };

/** The structure a `query` handler must return. */
type IdmQueryResult = {
  result: IdmResource[];
  resultCount?: number;
  pagedResultsCookie?: string | null;
  totalPagedResults?: number;
  remainingPagedResults?: number;
  totalPagedResultsPolicy?: string;
};
