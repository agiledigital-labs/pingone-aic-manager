import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { repoRoot } from "../paths.ts";
import type { EnvProfile } from "./types.ts";
import { ProfileShapeError } from "./normalise.ts";

/**
 * Profiles live under the script-sync workspace, which `.gitignore` covers in
 * full. Managed object and property names are client business vocabulary and
 * the sensitive-metadata scanner deliberately holds no client-name denylist —
 * so the guard here is the location, not a filter.
 */
export function profilePath(tenant: string, root = repoRoot): string {
  return join(root, "workspace", tenant, "harness-profile.json");
}

export function writeProfile(profile: EnvProfile, root = repoRoot): string {
  const path = profilePath(profile.tenant, root);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(profile, null, 2)}\n`, {
    encoding: "utf8",
    mode: 0o600,
  });
  return path;
}

export function readProfile(tenant: string, root = repoRoot): EnvProfile {
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
  root = repoRoot
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
