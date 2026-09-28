import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  ensureStateSubdir,
  failuresPath,
  latestLogsPath,
  PROJECT_ENV,
  projectRoot,
  STATE_DIR_ENV,
  stateDir,
} from "../src/project.ts";
import { profilePath, readProfile, writeProfile } from "../src/profile/store.ts";
import type { EnvProfile } from "../src/profile/types.ts";

let dir: string;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "rhino-local-project-"));
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe("projectRoot", () => {
  it("prefers RHINO_LOCAL_PROJECT", () => {
    mkdirSync(join(dir, "repo", ".git"), { recursive: true });
    expect(projectRoot({ [PROJECT_ENV]: join(dir, "elsewhere") }, join(dir, "repo"))).toBe(
      join(dir, "elsewhere")
    );
  });

  it("walks up to the nearest .git directory", () => {
    mkdirSync(join(dir, "repo", ".git"), { recursive: true });
    mkdirSync(join(dir, "repo", "a", "b"), { recursive: true });
    expect(projectRoot({}, join(dir, "repo", "a", "b"))).toBe(join(dir, "repo"));
  });

  it("stops at a worktree's .git file, the nearer of two", () => {
    mkdirSync(join(dir, ".git"));
    mkdirSync(join(dir, "wt", "sub"), { recursive: true });
    writeFileSync(join(dir, "wt", ".git"), "gitdir: /elsewhere\n");
    expect(projectRoot({}, join(dir, "wt", "sub"))).toBe(join(dir, "wt"));
  });

  it("ignores an empty override", () => {
    mkdirSync(join(dir, ".git"));
    expect(projectRoot({ [PROJECT_ENV]: "" }, dir)).toBe(dir);
  });
});

describe("stateDir", () => {
  it("is <project>/.rhino-local, or RHINO_LOCAL_STATE_DIR outright", () => {
    expect(stateDir({ [PROJECT_ENV]: dir })).toBe(join(dir, ".rhino-local"));
    expect(stateDir({ [PROJECT_ENV]: dir, [STATE_DIR_ENV]: join(dir, "s") })).toBe(join(dir, "s"));
  });
});

describe("ensureStateSubdir", () => {
  it("creates a 0700 directory that ignores itself, and keeps an existing ignore", () => {
    const root = join(dir, ".rhino-local");
    const sub = ensureStateSubdir(root, "failures");
    expect(sub).toBe(join(root, "failures"));
    expect(readFileSync(join(root, ".gitignore"), "utf8")).toBe("*\n");
    expect(statSync(root).mode & 0o777).toBe(0o700);
    expect(statSync(sub).mode & 0o777).toBe(0o700);

    writeFileSync(join(root, ".gitignore"), "*\n# mine\n");
    ensureStateSubdir(root, "profiles");
    expect(readFileSync(join(root, ".gitignore"), "utf8")).toBe("*\n# mine\n");
  });

  it("places the failure dump and show-log view under the state directory", () => {
    const env = { [STATE_DIR_ENV]: join(dir, "s") };
    expect(failuresPath({ env })).toBe(join(dir, "s", "failures", "failures.jsonl"));
    // A read path does not create anything.
    expect(() => statSync(join(dir, "s"))).toThrow();
    expect(latestLogsPath(env)).toBe(join(dir, "s", "failures", "latest-logs.json"));
    expect(readFileSync(join(dir, "s", ".gitignore"), "utf8")).toBe("*\n");
  });
});

describe("profile store", () => {
  const profile = { tenant: "sandbox", objects: [] } as unknown as EnvProfile;

  it("writes profiles/<tenant>.json 0600 inside a self-ignoring state directory", () => {
    const root = join(dir, "state");
    const path = writeProfile(profile, root);
    expect(path).toBe(join(root, "profiles", "sandbox.json"));
    expect(statSync(path).mode & 0o777).toBe(0o600);
    expect(readFileSync(join(root, ".gitignore"), "utf8")).toBe("*\n");
    expect(readProfile("sandbox", root)).toMatchObject({ tenant: "sandbox" });
  });

  it.each(["../escape", "a/b", "", ".hidden"])("refuses tenant name %j", (tenant) => {
    expect(() => profilePath(tenant, dir)).toThrow(/cannot name a profile file/);
  });
});
