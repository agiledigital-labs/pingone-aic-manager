import { chmodSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ensureStateSubdir, stateDir } from "../project.ts";
import type { EnvProfile } from "./types.ts";
import { ProfileShapeError } from "./normalise.ts";

/**
 * Profiles live in the harness state directory, which ignores itself.
 * Managed object and property names are client business vocabulary, and the
 * sensitive-metadata scanner deliberately holds no client-name denylist, so
 * the guard is the location, not a filter.
 */
export function profilePath(tenant: string, root = stateDir()): string {
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(tenant)) {
    throw new ProfileShapeError(
      `tenant name ${JSON.stringify(tenant)} cannot name a profile file`
    );
  }
  return join(root, "profiles", `${tenant}.json`);
}

export function writeProfile(profile: EnvProfile, root = stateDir()): string {
  const path = profilePath(profile.tenant, root);
  ensureStateSubdir(root, "profiles");
  writeFileSync(path, `${JSON.stringify(profile, null, 2)}\n`, {
    encoding: "utf8",
    mode: 0o600,
  });
  // `mode` applies only when the write creates the file.
  chmodSync(path, 0o600);
  return path;
}

export function readProfile(tenant: string, root = stateDir()): EnvProfile {
  const path = profilePath(tenant, root);
  if (!existsSync(path)) {
    throw new ProfileShapeError(
      `no environment profile for ${JSON.stringify(tenant)} at ${path}. Pull one: npm run pull-profile -- --tenant ${tenant}`
    );
  }
  return parseProfile(readFileSync(path, "utf8"), path);
}

/** Read a profile if one exists; absent means "run without schema checking". */
export function tryReadProfile(
  tenant: string,
  root = stateDir()
): EnvProfile | undefined {
  return existsSync(profilePath(tenant, root))
    ? readProfile(tenant, root)
    : undefined;
}

export function parseProfile(text: string, path: string): EnvProfile {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new ProfileShapeError(`profile at ${path} is not JSON: ${message}`);
  }
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    typeof (parsed as EnvProfile).tenant !== "string" ||
    typeof (parsed as EnvProfile).objects !== "object"
  ) {
    throw new ProfileShapeError(`profile at ${path} is missing tenant/objects`);
  }
  return parsed as EnvProfile;
}
