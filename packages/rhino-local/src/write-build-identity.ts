/**
 * Record the commit of the tree being packed. Run from `npm run build`
 * (which `prepack` calls), after `dist/` exists. A checkout that never
 * builds has no file, and the runtime then prints `(commit unknown)`
 * without asking git.
 */
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { buildIdentityPath, packageRoot } from "./paths.ts";

function git(args: string[]): string {
  return execFileSync("git", args, { cwd: packageRoot, encoding: "utf8" }).trim();
}

const commit = git(["rev-parse", "--short=12", "HEAD"]);
if (!/^[0-9a-f]{7,}$/.test(commit)) {
  throw new Error(`rhino-local: git rev-parse --short=12 HEAD returned ${JSON.stringify(commit)}`);
}
const dirty = git(["status", "--porcelain"]) !== "";
writeFileSync(buildIdentityPath, `${JSON.stringify({ commit, dirty })}\n`);
process.stdout.write(`wrote ${buildIdentityPath} (${commit}${dirty ? "-dirty" : ""})\n`);
