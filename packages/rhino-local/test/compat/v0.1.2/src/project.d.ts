/** Overrides {@link projectRoot}. */
export declare const PROJECT_ENV = "AIC_SCRIPT_TESTER_PROJECT";
/** Overrides {@link stateDir}. */
export declare const STATE_DIR_ENV = "AIC_SCRIPT_TESTER_STATE_DIR";
/**
 * The project the harness is running for: `AIC_SCRIPT_TESTER_PROJECT`, else the
 * nearest ancestor of the working directory holding `.git` (a directory, or
 * the file a worktree has), else the working directory.
 *
 * Not the package's own location — installed, that is inside
 * `node_modules`, and nothing the harness writes belongs there.
 */
export declare function projectRoot(env?: NodeJS.ProcessEnv, cwd?: string): string;
/**
 * Where the harness keeps per-project state: failure dumps, show-log views,
 * environment profiles. `AIC_SCRIPT_TESTER_STATE_DIR`, else
 * `<project>/.aic-script-tester`.
 */
export declare function stateDir(env?: NodeJS.ProcessEnv, cwd?: string): string;
/**
 * Create `dir` (under the state directory) and make sure the state directory
 * ignores itself. Everything in it can name a live tenant — log bodies,
 * hostnames, managed-object vocabulary — and a consuming repo's `.gitignore`
 * knows nothing about it, so the guard travels with the directory rather than
 * relying on the host repo (the same move as `src/backup.rs`).
 */
export declare function ensureStateSubdir(root: string, ...parts: string[]): string;
/**
 * `<state>/failures/failures.jsonl`. `create` makes the directory (and the
 * state directory's `.gitignore`); a read does not.
 */
export declare function failuresPath(options?: {
    create?: boolean;
    env?: NodeJS.ProcessEnv;
}): string;
/** `<state>/failures/latest-logs.json`, the view `show-log` opens. */
export declare function latestLogsPath(env?: NodeJS.ProcessEnv): string;
