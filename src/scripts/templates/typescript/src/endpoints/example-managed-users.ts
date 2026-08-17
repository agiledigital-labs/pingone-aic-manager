// Demo endpoint: the TENANT-DERIVED types.
//
// The other two examples show routing, validation and code sharing over
// in-memory fixtures. This one shows what `aic workspace update` adds on top of
// them. It writes `src/generated/managed.ts` from your tenant's managed schema:
// one exported interface per object, plus a `declare global` map that the
// `openidm` signatures in `framework/idm-globals.d.ts` key their result types
// on. So `openidm.read(...)` hands back YOUR schema rather than an opaque
// `Record<string, unknown>`, and a misspelled field is a build error.
//
// THREE THINGS TO KNOW.
//
// 1. Path inference needs a TEMPLATE LITERAL. openidm.read(`managed/alpha_user/${id}`)
//    infers the argument as the type `managed/alpha_user/${string}`, which
//    resolves to StoredRecord<AlphaUser>. openidm.read("managed/alpha_user/" + id)
//    infers plain `string` — there is nothing to look the object up by, so the
//    call degrades to the generic CrestResource. Both compile; only one checks
//    your field names. Babel downlevels the backticks to concatenation, so the
//    emitted ES5 is identical either way.
//
// 2. Missing tenant types cost inference, never a compile error — every
//    conditional resolves to `never` and each signature falls back to its
//    untyped form. If hovering below shows CrestResource instead of AlphaUser,
//    run `aic workspace update` and reload your editor's TS server.
//
// 3. This file is TENANT-SHAPED. It assumes `managed/alpha_user` with the stock
//    AIC properties (`userName`, `mail`, `givenName`, `sn`, `accountStatus`,
//    `telephoneNumber`, `manager`). If your tenant's schema differs,
//    `npm run type-check` names the mismatch — adapt the file or delete it.
//
// At runtime it only READS. The write verbs are typed in `writeTypingTour`
// below, which nothing calls, so building and pushing this endpoint cannot
// modify a user. The "SHOULD NOT COMPILE" block at the bottom lists the
// negative cases worth trying in your editor.
//
// User file — seeded once, yours to change.

import {
  defineEndpoint,
  notFound,
  queryResult,
  queryResultSchema,
  route,
  v,
  type JsonSchema,
} from "../../framework/index.ts";
import type { AlphaUser } from "../generated/managed.ts";

/**
 * A collection path checked against the generated map. `ManagedName` is
 * `keyof ManagedObjects & string`, and it is global — no import needed. Change
 * this to `"managed/alpha_users"` to watch it fail.
 */
export const USER_COLLECTION: ManagedName = "managed/alpha_user";

const ACCOUNT_STATUSES = ["active", "inactive"] as const;

const USER_RESPONSE: JsonSchema = v.object(
  {
    _id: v.string({ description: "Managed user id." }),
    userName: v.string(),
    mail: v.string(),
    displayName: v.string({ description: "givenName + sn." }),
    hasManager: v.boolean(),
  },
  { description: "The projection of a managed user this endpoint returns." }
).schema;

const CONTACT_RESPONSE: JsonSchema = v.object(
  {
    _id: v.string(),
    userName: v.string(),
    mail: v.string(),
    telephoneNumber: v.string({
      description: "Empty when the user has none.",
    }),
    managerRef: v.string({
      description: "Manager's _ref; empty when unset.",
    }),
  },
  { description: "Contact details, read with an explicit field selector." }
).schema;

export default defineEndpoint({
  name: "example-managed-users",
  summary: "Managed users, read through the generated tenant types (demo)",
  headers: { "x-request-id": v.optional(v.uuid()) },
  routes: [
    route({
      method: "read",
      path: "/{userId}",
      summary: "Read one managed user.",
      params: { userId: v.uuid("Managed user id.") },
      response: USER_RESPONSE,
      handler: ({ params, log }) => {
        // Hover `user`: StoredRecord<AlphaUser> | null.
        const user = openidm.read(`managed/alpha_user/${params.userId}`);
        if (user === null) {
          throw notFound("No such user", { userId: params.userId });
        }
        // `idOf` demands the StoredRecord form, so this line is also the proof
        // that a plain read carries `_id` as `string` rather than
        // `string | undefined`.
        log.debug("read user", { id: idOf(user) });
        return summarise(user);
      },
    }),

    route({
      method: "read",
      path: "/{userId}/contact",
      summary: "Read one user's contact details with an explicit selector.",
      params: { userId: v.uuid("Managed user id.") },
      response: CONTACT_RESPONSE,
      handler: ({ params }) => {
        // The three-argument form checks `fields` against the schema: a typo is
        // a build error WITH a spelling suggestion, and relationship / `_meta`
        // paths are accepted (docs/api/10-managed-objects.md).
        const user = openidm.read(`managed/alpha_user/${params.userId}`, null, [
          "userName",
          "mail",
          "telephoneNumber",
          "manager/userName",
          "_meta/lastChanged",
        ]);
        if (user === null) {
          throw notFound("No such user", { userId: params.userId });
        }
        // Note the hover here: `AlphaUser | null`, WITHOUT the StoredRecord
        // guarantee. Restricting the returned fields has not been verified to
        // still include `_id`/`_rev`, so this overload does not promise them —
        // which is why the id below comes from the path, not the record.
        return {
          _id: params.userId,
          userName: user.userName,
          mail: user.mail,
          telephoneNumber: user.telephoneNumber ?? "",
          managerRef: user.manager?._ref ?? "",
        };
      },
    }),

    route({
      method: "query",
      path: "/",
      summary: "Page through managed users by account status.",
      query: {
        status: v.withDefault(v.enumOf(ACCOUNT_STATUSES), "active"),
        pageSize: v.withDefault(
          v.integer({ min: 1, max: 200, description: "Records per page." }),
          50
        ),
      },
      response: queryResultSchema(USER_RESPONSE),
      handler: ({ query, log }) => {
        // `_queryFilter` is REQUIRED by `QueryParams` — IDM rejects a query
        // without one, so omitting it fails the build instead of the request.
        // Interpolating `query.status` is safe because `v.enumOf` has already
        // pinned it to one of two literals; never interpolate a free string.
        const page = openidm.query(USER_COLLECTION, {
          _queryFilter: `/accountStatus eq "${query.status}"`,
          _sortKeys: "userName",
          _pageSize: query.pageSize,
        });
        // `page.result` is AlphaUser[].
        log.debug("queried users", { count: page.resultCount });
        return queryResult(page.result.map(summarise));
      },
    }),
  ],
});

/** Only a stored record satisfies this — see the call site in the read route. */
function idOf(user: StoredRecord<AlphaUser>): string {
  return user._id;
}

/**
 * Takes the bare interface rather than `StoredRecord<AlphaUser>`, because a
 * `query` result is typed as `AlphaUser[]`: a query can carry `_fields`, so the
 * `_id`/`_rev` guarantee that a plain `read` gets does not apply. That is the
 * one asymmetry worth remembering, and the `?? ""` below is its cost.
 */
function summarise(user: AlphaUser): {
  _id: string;
  userName: string;
  mail: string;
  displayName: string;
  hasManager: boolean;
} {
  return {
    _id: user._id ?? "",
    userName: user.userName,
    mail: user.mail,
    displayName: user.givenName + " " + user.sn,
    hasManager: user.manager !== undefined,
  };
}

/**
 * The write verbs and the fallbacks, typed but NEVER CALLED. It exists so the
 * whole `openidm` surface is covered by `npm run type-check` and so you can
 * hover each binding in one place; no route reaches it, so pushing this
 * endpoint cannot write to a user.
 */
export function writeTypingTour(userId: string): Record<string, unknown> {
  // `content` is Partial<AlphaUser>: a create need not carry every required
  // property as far as the TYPES are concerned (IDM still enforces its own
  // schema), and an unknown key or a wrong value type is a build error.
  const created = openidm.create(USER_COLLECTION, null, {
    userName: "demo.user",
    mail: "demo.user@example.invalid",
    givenName: "Demo",
    sn: "User",
  });

  // Same Partial<AlphaUser> content; returns AlphaUser.
  const updated = openidm.update(`managed/alpha_user/${userId}`, null, {
    telephoneNumber: "+61 2 0000 0000",
  });

  // `operations` is PatchOperation[] — `field` is a JSON pointer, and the
  // operation name is a closed union, so "replce" does not compile.
  const patched = openidm.patch(`managed/alpha_user/${userId}`, null, [
    { operation: "replace", field: "/description", value: "patched" },
  ]);

  const removed = openidm.delete(`managed/alpha_user/${userId}`, null);

  // An unknown path degrades to CrestResource: `_id`/`_rev` optional, every
  // other key reachable only through an index (`system["uid"]`), because
  // `noPropertyAccessFromIndexSignature` is on. That indexing friction is the
  // signal that you are off the typed path.
  const system = openidm.read(`system/ldap/account/${userId}`);

  // Actions are untyped by nature — one name, one payload shape per action —
  // so the result is `unknown` and narrowing it is your job.
  const sent = openidm.action("external/email", "send", {
    to: "demo.user@example.invalid",
  });

  // ESV / boot property lookup. `null` when the property is not set.
  const region = identityServer.getProperty("esv.demo.region", "au");

  return { created, updated, patched, removed, system, sent, region };
}

// ---------------------------------------------------------------------------
// SHOULD NOT COMPILE — uncomment one at a time to watch the types bite. The
// expected message is quoted; if you get "any" or no error at all, the
// generated types are missing (see note 2 in the header).
// ---------------------------------------------------------------------------
//
// openidm.read(`managed/alpha_user/${userId}`).nosuchField;
//   Property 'nosuchField' does not exist on type 'StoredRecord<AlphaUser>'.
//
// openidm.read(`managed/alpha_user/${userId}`, null, ["userNmae"]);
//   Type '"userNmae"' is not assignable to type 'ManagedField<AlphaUser>'.
//   Did you mean '"userName"'?
//
// openidm.query(USER_COLLECTION, { _pageSize: 5 });
//   Property '_queryFilter' is missing ... but required in type 'QueryParams'.
//
// openidm.create(USER_COLLECTION, null, { userName: 42 });
//   Type 'number' is not assignable to type 'string'.
//
// openidm.read(`managed/no_such_object/${userId}`, null, ["userName"]);
//   Argument of type 'string[]' is not assignable to parameter of type 'never'.
//   (An unknown managed path REJECTS a field list: there is nothing to check
//   it against. Non-managed paths still accept a free-form string[].)
//
// const untyped: ManagedName = "managed/alpha_users";
//   Type '"managed/alpha_users"' is not assignable to type 'ManagedName'.
//   Did you mean '"managed/alpha_user"'?
