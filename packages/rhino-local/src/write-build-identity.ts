/**
 * Record the commit of the tree being packed. Run from `npm run build`
 * (which `prepack` calls), after `dist/` exists. No git metadata — no
 * `.git`, or git itself absent — leaves the file unwritten. A checkout that
 * never builds has no file either, and the runtime then prints
 * `(commit unknown)` without asking git.
 */
import { execFileSync } from "node:child_process";
import { existsSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { buildIdentityPath, packageRoot } from "./paths.ts";

function git(root: string, env: NodeJS.ProcessEnv, args: readonly string[]): string {
  return execFileSync("git", args, {
    cwd: root,
    env,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

/** Read the identity of the checkout that tracks this package's manifest. */
export function readCheckoutIdentity(root: string): { commit: string; dirty: boolean } | undefined {
  // Discover the repository from the package directory. Inherited Git
  // settings can redirect discovery, the worktree, the index or object and config reads.
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith("GIT_")),
  );
  let commit: string;
  let dirty: boolean;
  try {
    git(root, env, ["ls-files", "--error-unmatch", "--", "package.json"]);
    commit = git(root, env, ["rev-parse", "--short=12", "HEAD"]);
    dirty = git(root, env, ["status", "--porcelain", "--untracked-files=normal", "--", "."]) !== "";
  } catch {
    return undefined;
  }
  return { commit, dirty };
}

function identityPath(root: string): string {
  return root === packageRoot ? buildIdentityPath : join(root, "dist", "build-identity.json");
}

export type BuildIdentityWrite =
  | { status: "written"; commit: string; dirty: boolean; path: string }
  | { status: "unknown"; path: string };

/**
 * Write `dist/build-identity.json` for `root`, or remove a stale file when
 * git cannot name the tree. `prepack` calls this with the package root.
 */
export function writeBuildIdentity(root: string = packageRoot): BuildIdentityWrite {
  const path = identityPath(root);
  const identity = readCheckoutIdentity(root);
  if (identity === undefined) {
    if (existsSync(path)) rmSync(path);
    return { status: "unknown", path };
  }
  const { commit, dirty } = identity;
  if (!/^[0-9a-f]{7,}$/.test(commit)) {
    throw new Error(`rhino-local: git rev-parse --short=12 HEAD returned ${JSON.stringify(commit)}`);
  }
  writeFileSync(path, `${JSON.stringify({ commit, dirty })}\n`);
  return { status: "written", commit, dirty, path };
}

const entry = process.argv[1];
if (entry && import.meta.url === pathToFileURL(entry).href) {
  const result = writeBuildIdentity();
  if (result.status === "unknown") {
    process.stdout.write("build identity unknown (git metadata unavailable)\n");
  } else {
    process.stdout.write(`wrote ${result.path} (${result.commit}${result.dirty ? "-dirty" : ""})\n`);
  }
}
