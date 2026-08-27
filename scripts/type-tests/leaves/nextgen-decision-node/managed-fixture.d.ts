// Stand-in for the managed-object declarations `aic workspace update` writes
// from a live tenant (`types/managed/<object>.d.ts` + `openidm-map.d.ts`).
//
// The type-test leaves need one because `interface ManagedObjects` ships EMPTY:
// with nothing merged into it, every conditional in the `openidm` signatures
// takes its unknown-path fallback and the projection machinery under test is
// never instantiated. A leaf without this file parses `Projected`,
// `SelectedMembers` and `ExpansionOf` and exercises none of them.
//
// A FIXTURE, not a copy of a tenant: it pins one relationship of each
// cardinality and one optional scalar, which no particular tenant is obliged to
// have. Mirrors the generated shape — `RelationshipRef` as `_shared.d.ts`
// emits it, `_id`/`_rev` optional exactly as `render_object` leaves them,
// because the same interface types an onCreate draft that has neither yet.
//
// The name is deliberately un-tenant-like so it cannot collide with a real
// object and trip declaration merging.

interface RelationshipRef {
  _ref: string;
  _refResourceCollection?: string;
  _refResourceId?: string;
  _refProperties?: { _id?: string; _rev?: string } & Record<string, any>;
}

interface AicFixtureUser {
  _id?: string;
  _rev?: string;
  userName: string;
  sn: string;
  mail: string;
  /** Optional in the schema, so a projection returns it as `string | null`. */
  telephoneNumber?: string;
  /** Single-valued: unset comes back as `null`, never absent. */
  manager?: RelationshipRef;
  /** Multi-valued: unset comes back as `[]`, never `null`. */
  authzRoles?: RelationshipRef[];
}

interface ManagedObjects {
  "managed/__aic_fixture_user": AicFixtureUser;
}
