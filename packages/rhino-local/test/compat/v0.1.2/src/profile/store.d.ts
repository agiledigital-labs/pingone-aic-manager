import type { EnvProfile } from "./types.ts";
/**
 * Profiles live in the harness state directory, which ignores itself.
 * Managed object and property names are client business vocabulary, and the
 * sensitive-metadata scanner deliberately holds no client-name denylist, so
 * the guard is the location, not a filter.
 */
export declare function profilePath(tenant: string, root?: string): string;
export declare function writeProfile(profile: EnvProfile, root?: string): string;
export declare function readProfile(tenant: string, root?: string): EnvProfile;
/** Read a profile if one exists; absent means "run without schema checking". */
export declare function tryReadProfile(tenant: string, root?: string): EnvProfile | undefined;
export declare function parseProfile(text: string, path: string): EnvProfile;
