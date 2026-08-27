# Script-workspace type tests

The `.d.ts` files under `src/scripts/templates/` are shipped into every
scaffolded script workspace, and some of them carry real logic — conditional
types that count `{}` placeholders, project `fields` selections, or narrow a
managed record. None of that is exercised by `cargo test`: the Rust gates check
the strings that emit those files, never what a TypeScript compiler makes of
them.

This directory closes that gap. It assembles a declaration SUBSET of each leaf
`src/scripts/am.rs::leaf_tsconfig` and its IDM counterparts build, points `tsc`
at it, and asserts BOTH directions:

- `accept.cjs` — every call that is correct at runtime must compile clean.
- `reject.cjs` — every call that is a runtime defect must fail, and the
  `// expect:` comment on the line names the diagnostic code expected there.

A reject file that compiles clean is a failure, and so is one that fails on a
line with no `// expect:` marker — a type that rejects the wrong thing is worse
than one that rejects nothing, because it breaks a workspace of client scripts
that used to build.

## What a leaf is

One directory per script family, holding four files:

- `workspace` — `am` or `idm`; the two declaration sets redeclare the same
  global names and cannot share a program.
- `types` — the declaration files to include, one per line. A SUBSET of the
  include set the matching Rust `leaf_tsconfig` emits; the Rust test
  `type_test_leaf_manifests_are_subsets_of_the_real_leaf_configs` fails if a
  manifest names a declaration the real leaf does not, so a leaf here can be
  smaller than a shipped one but never a fiction.
- `accept.cjs` / `reject.cjs` — the two directions.

A reject row does not always prove what its comment says. The parity row
(`"double \\\\{} bound"` with two arguments) fails whether the counter finds one
slot or zero, so it is the matching ACCEPT row — the same string with ONE
argument — that actually pins the behaviour. When a reject case is
non-discriminating on its own, pair it and say so.

A leaf's `types` may **omit** declarations that pull in npm type packages
(`idm-libs.d.ts` wants `lodash`, `handlebars` and `validator`). Those add
nothing to the conditional types under test and would make the gate depend on
three pinned third-party versions. If a future test needs one, install the
`@types` packages the shipped `package.json` already names rather than
weakening the leaf.

## Run

```sh
scripts/type-tests/run.sh
```

Needs `node` and `tsc` on `PATH` (`npx typescript` is used if `tsc` is absent).
CI runs it in the `script workspace types` job.

## Managed-record projection

The other shipped declaration set carrying real logic — path resolution,
`fields` projection, `SelectedMembers`, relationship-cardinality expansion,
`_meta`, `read`/`query` result shaping. It exists in THREE copies:
`am/types/nextgen-common.d.ts`, `idm/types/common.d.ts`, and the TypeScript
project's `framework/idm-globals.d.ts`.

The first two are covered by the `nextgen-decision-node` and `idm-endpoint`
leaves; the third by the TypeScript project's own `tests/openidm-types.test.ts`,
which CI now runs (`npm run type-check` in that directory) — it is a wall of
`@ts-expect-error` markers, and those mean nothing unless a compiler reads them.

Two things make that coverage real rather than nominal:

- **`managed-fixture.d.ts`.** `interface ManagedObjects` ships EMPTY, so a leaf
  without a fixture takes the unknown-path fallback in every conditional and
  instantiates none of the machinery. It merges in one managed object with one
  relationship of each cardinality and one schema-optional scalar.
- **Discriminating cases.** The first attempt guarded everything
  (`if (record.manager) { … }`), which compiles under a correct projection AND
  under a broken one — it passed while the single-valued-expansion type was
  mutated to always-present, the exact bug that once cost a live 500. The
  fixtures now pin the shapes instead: an unguarded read of a single-valued
  expansion must FAIL, a `/** @type {string | null} */` assignment pins the
  required-and-nullable projection, and an unguarded `.length` on a multi-valued
  one must PASS.

Drift between the two ambient copies needs no equality assertion: each leaf
carries its own fixtures, so mutating one copy fails that copy's leaf and leaves
the other green. Verified both ways. (The logger format types DO have a
byte-equality test, for a different reason — they are meant to be identical, and
the IDM copy has no runtime evidence behind it.)

## Not yet covered

`idm-libs.d.ts` (needs `lodash`, `handlebars` and `validator` types), the
generated `types/managed/hooks/*.d.ts` and `types/sync/*.d.ts` overlays, and the
`../lib/*` path alias that only next-gen leaves get. None of them carry
conditional types today; add coverage when one does.
