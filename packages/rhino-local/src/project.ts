import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

/** Overrides {@link projectRoot}. */
export const PROJECT_ENV = "RHINO_LOCAL_PROJECT";
/** Overrides {@link stateDir}. */
export const STATE_DIR_ENV = "RHINO_LOCAL_STATE_DIR";

/**
 * The project the harness is running for: `RHINO_LOCAL_PROJECT`, else the
 * nearest ancestor of the working directory holding `.git` (a directory, or
 * the file a worktree has), else the working directory.
 *
 * Not the package's own location — installed, that is inside
 * `node_modules`, and nothing the harness writes belongs there.
 */
export function projectRoot(
  env: NodeJS.ProcessEnv = process.env,
  cwd: string = process.cwd()
): string {
  const configured = env[PROJECT_ENV];
  if (configured !== undefined && configured !== "") {
    return resolve(configured);
  }
  let dir = resolve(cwd);
  for (;;) {
    if (existsSync(join(dir, ".git"))) {
      return dir;
    }
    const parent = dirname(dir);
    if (parent === dir) {
      return resolve(cwd);
    }
    dir = parent;
  }
}

/**
 * Where the harness keeps per-project state: failure dumps, show-log views,
 * environment profiles. `RHINO_LOCAL_STATE_DIR`, else
 * `<project>/.rhino-local`.
 */
export function stateDir(env: NodeJS.ProcessEnv = process.env, cwd?: string): string {
  const configured = env[STATE_DIR_ENV];
  if (configured !== undefined && configured !== "") {
    return resolve(configured);
  }
  return join(projectRoot(env, cwd), ".rhino-local");
}

/**
 * Create `dir` (under the state directory) and make sure the state directory
 * ignores itself. Everything in it can name a live tenant — log bodies,
 * hostnames, managed-object vocabulary — and a consuming repo's `.gitignore`
 * knows nothing about it, so the guard travels with the directory rather than
 * relying on the host repo (the same move as `src/backup.rs`).
 */
export function ensureStateSubdir(root: string, ...parts: string[]): string {
  mkdirSync(root, { recursive: true, mode: 0o700 });
  const ignore = join(root, ".gitignore");
  if (!existsSync(ignore)) {
    try {
      writeFileSync(ignore, "*\n", { encoding: "utf8", flag: "wx" });
    } catch (error) {
      // A concurrent writer got there first; its file says the same thing.
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") {
        throw error;
      }
    }
  }
  const dir = join(root, ...parts);
  mkdirSync(dir, { recursive: true, mode: 0o700 });
  return dir;
}

/**
 * `<state>/failures/failures.jsonl`. `create` makes the directory (and the
 * state directory's `.gitignore`); a read does not.
 */
export function failuresPath(options: { create?: boolean; env?: NodeJS.ProcessEnv } = {}): string {
  const root = stateDir(options.env);
  const dir = options.create === true ? ensureStateSubdir(root, "failures") : join(root, "failures");
  return join(dir, "failures.jsonl");
}

/** `<state>/failures/latest-logs.json`, the view `show-log` opens. */
export function latestLogsPath(env: NodeJS.ProcessEnv = process.env): string {
  return join(ensureStateSubdir(stateDir(env), "failures"), "latest-logs.json");
}
