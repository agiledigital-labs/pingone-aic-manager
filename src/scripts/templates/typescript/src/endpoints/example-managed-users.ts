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
// FOUR THINGS TO KNOW.
//
// 1. Path inference needs a TEMPLATE LITERAL. openidm.read(`managed/alpha_user/${id}`)
//    infers the argument as the type `managed/alpha_user/${string}`, which
//    resolves to StoredRecord<AlphaUser>. openidm.read("managed/alpha_user/" + id)
//    infers plain `string` — there is nothing to look the object up by, so the
//    call degrades to the generic CrestResource. Both compile; only one checks
//    your field names. Babel downlevels the backticks to concatenation, so the
//    emitted ES5 is identical either way.
//
// 2. A `fields` list PROJECTS the result type: ask for two properties and the
//    other seventy are gone from the type, while `_id`/`_rev` stay because IDM
//    returns them whatever you ask for. `openidm.query` takes the same third
//    argument and projects every row. Full behaviour, including relationship and
//    `_meta` expansions, is pinned down in `tests/openidm-types.test.ts`.
//
// 3. Missing tenant types cost inference, never a compile error — every
//    conditional resolves to `never` and each signature falls back to its
//    untyped form. If hovering below shows CrestResource instead of AlphaUser,
//    run `aic workspace update` and reload your editor's TS server.
//
// 4. This file is TENANT-SHAPED. It assumes `managed/alpha_user` with the stock
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
  queryRoute,
  route,
  v,
} from "../../framework/index.ts";
import type { AlphaUser } from "../generated/managed.ts";

/**
 * A collection path checked against the generated map. `ManagedName` is
 * `keyof ManagedObjects & string`, and it is global — no import needed. Change
 * this to `"managed/alpha_users"` to watch it fail.
 */
export const USER_COLLECTION: ManagedName = "managed/alpha_user";

const ACCOUNT_STATUSES = ["active", "inactive"] as const;

const USER_RESPONSE = v.object(
  {
    _id: v.string({ description: "Managed user id." }),
    userName: v.string(),
    mail: v.string(),
    displayName: v.string({ description: "givenName + sn." }),
    hasManager: v.boolean(),
  },
  { description: "The projection of a managed user this endpoint returns." }
);

const CONTACT_RESPONSE = v.object(
  {
    _id: v.string(),
    userName: v.string(),
    mail: v.string(),
    // ABSENT and NULL are different, and this route meets both. A schema
    // property the user has not filled in is simply missing from the projection,
    // which is `v.optional`; an unset single-valued RELATIONSHIP comes back as
    // an explicit `null`, which only `v.nullable` accepts. Collapsing either one
    // to `""` — as this example used to — invents a value the tenant never sent
    // and puts it in the OpenAPI document as a plain string.
    telephoneNumber: v.optional(
      v.string({ description: "Absent when the user has none." })
    ),
    managerRef: v.nullable(
      v.string({ description: "Manager's _ref; null when unset." })
    ),
  },
  { description: "Contact details, read with an explicit field selector." }
);

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
        // `user._id` is `string`, not `string | undefined`: the read overloads
        // return `StoredRecord<AlphaUser>`, which adds the guarantee the
        // generated interface leaves off for an onCreate draft.
        log.debug("read user", { id: user._id });
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
        // The three-argument form checks `fields` against the schema — a typo is
        // a build error with a spelling suggestion — and PROJECTS the result to
        // what was asked for.
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
        // Hover `user`: the four requested members plus `_id`/`_rev`, and
        // nothing else. `user.givenName` does not compile here — it was not
        // requested, so it is not in the response. `_id` IS available, because
        // IDM returns it whatever the field list says (verified 2026-08-17).
        //
        // `manager` is the relationship EXPANSION rather than the declared
        // `RelationshipRef`, since a `parent/child` path was asked for — and it
        // is NULLABLE, because an unset single-valued relationship comes back as
        // `null` (a multi-valued one comes back as `[]`). Dropping the `?.` below
        // was a live 500 on the first user without a manager, past every gate.
        // The target's schema is unknown to these types, so its members are
        // index-only: `manager["userName"]`, not `manager.userName`.
        return {
          _id: user._id,
          userName: user.userName,
          mail: user.mail,
          telephoneNumber: user.telephoneNumber,
          managerRef: user.manager?._ref ?? null,
        };
      },
    }),

    queryRoute({
      path: "/",
      summary: "Page through managed users by account status.",
      query: {
        status: v.withDefault(v.enumOf(ACCOUNT_STATUSES), "active"),
        pageSize: v.withDefault(
          v.integer({ min: 1, max: 200, description: "Records per page." }),
          50
        ),
      },
      response: USER_RESPONSE,
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
        // `page.result` is `StoredRecord<AlphaUser>[]` — pass a third `fields`
        // argument here too and every row is projected to it instead.
        log.debug("queried users", { count: page.resultCount });
        return queryResult(page.result.map(summarise));
      },
    }),
  ],
});

/**
 * Takes `StoredRecord<AlphaUser>` — the type both an unprojected read and an
 * unprojected query row have — so `_id` needs no `?? ""` fallback. Ask for a
 * `fields` list at either call site and the argument no longer fits, which is
 * the projection doing its job.
 */
function summarise(user: StoredRecord<AlphaUser>): {
  _id: string;
  userName: string;
  mail: string;
  displayName: string;
  hasManager: boolean;
} {
  return {
    _id: user._id,
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
// expected message is quoted; if you get no error at all, the generated types
// are missing (see note 3 in the header). `tests/openidm-types.test.ts` holds
// the same cases as `@ts-expect-error` assertions, so they are gated rather
// than just documented.
// ---------------------------------------------------------------------------
//
// openidm.read(`managed/alpha_user/${userId}`).nosuchField;
//   Property 'nosuchField' does not exist on type 'StoredRecord<AlphaUser>'.
//
// openidm.read(`managed/alpha_user/${userId}`, null, ["userNmae"]);
//   Type '"userNmae"' is not assignable to type 'ManagedField<AlphaUser>'.
//   Did you mean '"userName"'?
//
// openidm.read(`managed/alpha_user/${userId}`, null, ["userName"]).sn;
//   Property 'sn' does not exist on type 'StoredRecord<Pick<AlphaUser,
//   "userName">>'. — the projection dropped everything you did not ask for.
//
// openidm.query(USER_COLLECTION, { _pageSize: 5 });
//   Property '_queryFilter' is missing ... but required in type 'QueryParams'.
//
// openidm.query(USER_COLLECTION, { _queryFilter: "true" }, ["mial"]);
//   Type '"mial"' is not assignable to type 'ManagedField<AlphaUser>'.
//   Did you mean '"mail"'? — query takes a field list too, and projects rows.
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
//
// Delete a member from the object either read handler returns, e.g. `userName`
// from the `/contact` route:
//   Property 'userName' is missing in type '{ ... }' but required in type
//   '{ _id: string; userName: string; ... }'. — the handler's return value is
//   checked against the route's `response` validator, so the OpenAPI document
//   cannot promise a field the code stopped sending.
