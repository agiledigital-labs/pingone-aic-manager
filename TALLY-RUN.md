# DO NOT MERGE: emulator fault-tally run

This branch exists only to run the E19 fault tally of the unreleased emulator
build (`feat/emulator` at `7796fc8`) over a client project's tests, on a machine
that can only reach this repository. It must never be merged into `main`; it is
deleted when the run is done.

The run installs the branch's build of
`@agiledigital/pingone-aic-script-tester` into a **throwaway worktree** of the
client project, runs the suite once, and prints a pasteable summary: one row per
fault id with case, failed-case and file counts. No client file names leave the
machine.

## Prerequisites

- Node 24 or newer (`node --version`)
- JDK 25 (`javac -version`) — the package build compiles its Java runner
- read access to this GitHub repository

## 1. Build the package

```sh
git clone --depth 1 --branch tally/emulator-client-run \
  https://github.com/agiledigital-labs/pingone-aic-manager.git ~/aicm-emu
cd ~/aicm-emu && npm ci
cd packages/rhino-local && npm pack
ls ~/aicm-emu/packages/rhino-local/*.tgz
```

Expect `agiledigital-pingone-aic-script-tester-0.3.0.tgz` (the version number
is unchanged; the contents are the emulator build).

## 2. Throwaway worktree of the client project

```sh
cd <client-repo>
git worktree add --detach ../client-tally <branch with the unit tests>
cd ../client-tally/<directory whose package.json depends on the package>
npm ci
npm install --no-save vitest@5.0.5 \
  ~/aicm-emu/packages/rhino-local/agiledigital-pingone-aic-script-tester-0.3.0.tgz
```

The emulator build supports Vitest `~5.0.5` only.

## 3. Vitest config

The tally is written by the package's reporter, and the setup file refuses to
run without it. In the worktree's `vitest.config.*`, make sure `test` holds all
three package entries (append to any existing arrays; keep the client's own):

```ts
setupFiles: ["@agiledigital/pingone-aic-script-tester/vitest-setup"],
runner: "@agiledigital/pingone-aic-script-tester/vitest-runner",
reporters: ["default", "@agiledigital/pingone-aic-script-tester/vitest-reporter"],
```

If the config already sets its own `runner`, see "The supported Vitest
integration" in `packages/rhino-local/README.md` for subclassing.

## 4. Run

```sh
AIC_SCRIPT_TESTER_FAULT_TALLY="$PWD/tally.json" npx vitest run > run.log 2>&1
echo "exit=$?"
```

Many failures are expected: every fault fails the case that hit it, on purpose.

## 5. What to paste back

```sh
grep -E '^ *(Test Files|Tests|Errors|Duration) ' run.log
node -e '
const t = require(process.argv[1]);
for (const [k, rows] of [["fault", t.faults], ["unidentified", t.unidentified]])
  for (const r of rows) console.log([k, r.id, r.cases, r.failed, r.files.length].join("\t"));
' "$PWD/tally.json"
```

Paste both outputs. `unidentified` rows carry cause text instead of an id;
**read them before pasting** and replace anything that names the client
(endpoint, type or script names) with a placeholder.

If the run fails before any test starts (no `Test Files` line), paste the first
40 lines of `run.log` instead, with the same check.

## 6. Clean up

```sh
cd <client-repo> && git worktree remove --force ../client-tally
rm -rf ~/aicm-emu
```
