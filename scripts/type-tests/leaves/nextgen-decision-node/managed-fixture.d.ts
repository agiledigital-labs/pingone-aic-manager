// Stand-in for the managed-object declarations `aic workspace update` writes
// from a live tenant (`types/managed/_shared.d.ts`, `<object>.d.ts` and
// `openidm-map.d.ts`, all three collapsed into one file here).
//
// The type-test leaves need one because `interface ManagedObjects` ships EMPTY:
// with nothing merged into it, every conditional in the `openidm` signatures
// takes its unknown-path fallback and the projection machinery under test is
// never instantiated. A leaf without this file parses `Projected`,
// `SelectedMembers` and `ExpansionOf` and exercises none of them.
//
// This is a test augmentation, NOT a subset of a shipped template — the Rust
// subset assertion covers a leaf's `types` manifest and cannot see this file.
// `managed_fixture_matches_what_the_generator_emits` is what ties it to
// `managed_types.rs` instead, and it also holds the two copies identical.
//
// TWO objects, with disjoint properties. One is not enough: a fixture with a
// single managed object cannot tell correct path parsing in `ManagedRecordOf`
// from "return the only type there is".
//
// Names are deliberately un-tenant-like so they cannot collide with a real
// object and trip declaration merging.

interface RelationshipRef {
  _ref: string;
  _refResourceCollection?: string;
  _refResourceId?: string;
  _refProperties?: { _id?: string; _rev?: string } & Record<string, any>;
}

/** `_id`/`_rev` are optional exactly as `render_object` leaves them: the same
 * interface types an onCreate draft, which has neither yet. `StoredRecord` is
 * what puts them back on a read. */
interface AicFixtureUser {
  _id?: string;
  _rev?: string;
  userName: string;
  sn: string;
  mail: string;
  /** Schema-optional, so a projection returns it as `string | null`. */
  telephoneNumber?: string;
  /** Single-valued: unset comes back as `null`, never absent. */
  manager?: RelationshipRef;
  /** Multi-valued: unset comes back as `[]`, never `null`. */
  authzRoles?: RelationshipRef[];
}

/** Disjoint from AicFixtureUser on every property, so a record path that
 * resolves to the wrong interface fails rather than silently agreeing. */
interface AicFixtureDevice {
  _id?: string;
  _rev?: string;
  deviceId: string;
  model: string;
  serialNumber?: string;
  owner?: RelationshipRef;
}

interface ManagedObjects {
  "managed/__aic_fixture_user": AicFixtureUser;
  "managed/__aic_fixture_device": AicFixtureDevice;
}
