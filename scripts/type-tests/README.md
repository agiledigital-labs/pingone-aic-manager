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

## Not yet covered

The managed-record type machinery — path resolution, `fields` projection,
relationship expansion, `_meta`, `read`/`query` result shaping — is the other
shipped declaration set carrying real logic, in three copies
(`idm/types/common.d.ts`, `am/types/nextgen-common.d.ts`, and the TypeScript
project's `framework/idm-globals.d.ts`). The leaves here parse the first two but
with an EMPTY `ManagedObjects`, so none of its branches instantiate, and the
third is not compiled by this harness at all. The TypeScript project already has
a bidirectional test for it (`tests/openidm-types.test.ts`) that CI does not run.

Adding that is the next piece of work: run the TypeScript project's own
type-check in CI, then add a small generated-managed-object fixture here and
drive it through both the IDM endpoint and next-gen AM leaves.
