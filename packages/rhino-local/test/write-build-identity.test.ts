/**
 * `prepack` runs the build-identity writer. A tree with no git metadata must
 * still pack: the writer leaves the identity file unwritten, and the runtime
 * prints `(commit unknown)`.
 */
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { writeBuildIdentity } from "../src/write-build-identity.ts";

const roots: string[] = [];

function tree(): string {
  const root = mkdtempSync(join(tmpdir(), "rl-identity-"));
  roots.push(root);
  mkdirSync(join(root, "dist"));
  writeFileSync(join(root, "package.json"), "{\"name\":\"example\",\"version\":\"0.0.0\"}\n");
  return root;
}

function gitEnv(): NodeJS.ProcessEnv {
  const env = { ...process.env };
  delete env.GIT_DIR;
  delete env.GIT_WORK_TREE;
  delete env.GIT_INDEX_FILE;
  env.GIT_AUTHOR_NAME = "dev";
  env.GIT_AUTHOR_EMAIL = "dev@example.com";
  env.GIT_COMMITTER_NAME = "dev";
  env.GIT_COMMITTER_EMAIL = "dev@example.com";
  env.GIT_CONFIG_COUNT = "1";
  env.GIT_CONFIG_KEY_0 = "commit.gpgsign";
  env.GIT_CONFIG_VALUE_0 = "false";
  return env;
}

function git(root: string, args: readonly string[]): string {
  const result = spawnSync("git", [...args], { cwd: root, env: gitEnv(), encoding: "utf8" });
  expect(result.status, result.stderr).toBe(0);
  return result.stdout;
}

describe("writeBuildIdentity", () => {
  afterEach(() => {
    for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
  });

  it("leaves an unknown identity when the source tree has no git metadata", () => {
    const enclosing = mkdtempSync(join(tmpdir(), "rl-enclosing-"));
    roots.push(enclosing);
    git(enclosing, ["init", "-q"]);
    writeFileSync(join(enclosing, "README.md"), "enclosing repository\n");
    git(enclosing, ["add", "README.md"]);
    git(enclosing, ["commit", "-q", "-m", "init"]);
    const root = join(enclosing, "unpacked-package");
    mkdirSync(root);
    mkdirSync(join(root, "dist"));
    writeFileSync(join(root, "package.json"), "{\"name\":\"example\"}\n");
    const identity = join(root, "dist", "build-identity.json");
    writeFileSync(identity, "{\"commit\":\"0123456789ab\",\"dirty\":false}\n");
    expect(writeBuildIdentity(root)).toEqual({ status: "unknown", path: identity });
    expect(existsSync(identity)).toBe(false);
  });

  it("writes the commit of a git checkout", () => {
    const root = tree();
    writeFileSync(join(root, ".gitignore"), "dist\n");
    git(root, ["init", "-q"]);
    git(root, ["add", "package.json", ".gitignore"]);
    git(root, ["commit", "-q", "-m", "init"]);
    const commit = git(root, ["rev-parse", "--short=12", "HEAD"]).trim();
    const written = writeBuildIdentity(root);
    expect(written).toMatchObject({ status: "written", commit, dirty: false });
    expect(JSON.parse(readFileSync(join(root, "dist", "build-identity.json"), "utf8"))).toEqual({
      commit,
      dirty: false,
    });
  });

  it("scopes dirty state to the package and counts package untracked files", () => {
    const enclosing = mkdtempSync(join(tmpdir(), "rl-monorepo-"));
    roots.push(enclosing);
    const root = join(enclosing, "packages", "rhino-local");
    mkdirSync(join(root, "dist"), { recursive: true });
    writeFileSync(join(root, "package.json"), "{\"name\":\"example\"}\n");
    writeFileSync(join(root, ".gitignore"), "dist\n");
    git(enclosing, ["init", "-q"]);
    git(enclosing, ["add", "packages/rhino-local/package.json", "packages/rhino-local/.gitignore"]);
    git(enclosing, ["commit", "-q", "-m", "init"]);

    writeFileSync(join(enclosing, "notes.txt"), "outside package\n");
    expect(writeBuildIdentity(root)).toMatchObject({ status: "written", dirty: false });

    writeFileSync(join(root, "inside.txt"), "inside package\n");
    expect(writeBuildIdentity(root)).toMatchObject({ status: "written", dirty: true });
  });
});
