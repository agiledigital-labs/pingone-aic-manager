// IDM managed-object hook shared bindings. Layered on rhino + common (which
// declare `openidm`, `logger`, `context`, and `identityServer`). `object`, `oldObject`, and
// `newObject` are declared per object in generated
// `types/managed/<object>.d.ts` files.
//
// Provenance: live binding probe on the sandbox 2026-06-13 via temporary
// onCreate/onUpdate hooks (docs/api/10-managed-objects.md, raw dump in
// docs/api/bindings/managed-hooks-idm.json). onCreate/onUpdate are verified;
// onDelete/post* hooks are assumed to share this surface (open question).
//
// ⚠ Runtime gotchas (verified): any uncaught exception in a hook fails the
// caller's request with HTTP 500 and rolls back the write. Enumerating the
// scope (`for (var k in this)`) is itself fatal — never do it.

/** A single patch operation (for method:"patch" requests). */
interface ManagedHookPatchOperation {
  operation:
    "add" | "remove" | "replace" | "increment" | "move" | "copy" | "transform";
  field: string;
  value?: any;
  from?: string;
}

/**
 * The CREST request that triggered the hook. `method:"create"` and
 * `method:"update"` shapes verified live; the rest mirror the endpoint
 * request union and are unverified for hooks.
 */
type ManagedHookRequest =
  | {
      method: "create";
      fields: string[];
      resourcePath: string;
      additionalParameters: Record<string, string>;
      content: any;
      newResourceId: string | null;
    }
  | {
      method: "update";
      fields: string[];
      resourcePath: string;
      additionalParameters: Record<string, string>;
      revision?: string | null;
      content?: any;
    }
  | {
      method: "patch";
      fields: string[];
      resourcePath: string;
      additionalParameters: Record<string, string>;
      revision?: string | null;
      patchOperations?: ManagedHookPatchOperation[];
    }
  | {
      method: "delete";
      fields: string[];
      resourcePath: string;
      additionalParameters: Record<string, string>;
      revision?: string | null;
    };

declare const request: ManagedHookRequest;

/**
 * Java `org.forgerock.json.resource.ResourcePath` (the only Java-classed
 * binding — verified). Use `String(resourceName)`, which yields e.g.
 * `"managed/alpha_user/<id>"`.
 */
declare const resourceName: { toString(): string };
