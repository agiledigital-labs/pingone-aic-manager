/**
 * IDM's own runtime scripts (E15): the files under the tenant's
 * `/opt/openidm/bin/defaults/script` (and the engine's bundled `lib/`), which
 * IDM runs from a hook's `file` reference or a `require(id)`. Nothing here
 * ships them. The project supplies a provider — a directory, normally one
 * fetched from the tenant and committed, or a loader function — and a script
 * the provider does not supply is the harness fault `idm-runtime-script/<path>`.
 */
import { closeSync, constants, fstatSync, openSync, readdirSync, readFileSync, realpathSync, statSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { fileURLToPath } from "node:url";
import { emulationFailure } from "../emulation.ts";
import { missing } from "./errors.ts";
import type { Evidence } from "./provenance.ts";

/**
 * A directory whose files are named by IDM's relative script path
 * (`<dir>/roles/effectiveRoles.js`) — an absolute path or a `file:` URL, so
 * its base is explicit — or a loader answering that path with the script's
 * source, or `undefined` when it does not have it.
 */
export type RuntimeScriptProvider =
  | string
  | URL
  | ((scriptPath: string) => string | undefined | Promise<string | undefined>);

/** The fault-id family; the id is `idm-runtime-script/<path>`. */
export const RUNTIME_SCRIPT_FAULT = "idm-runtime-script";
/** A name the provider cannot be asked about: not canonical, a symbolic link, or not a regular file. */
export const RUNTIME_SCRIPT_PATH_FAULT = "idm-runtime-script-path";
/** The provider itself failed: a loader threw or returned a non-string, or a listed file could not be read. */
export const RUNTIME_SCRIPT_PROVIDER_FAULT = "idm-runtime-script-provider";

/**
 * How to get the scripts, named in every refusal. The `aic` command is the
 * planned fetch (E15 follow-on slice); when it ships, drop "planned" here.
 */
export const RUNTIME_SCRIPT_FETCH =
  "fetch the tenant's IDM runtime scripts into a directory committed beside the config, and pass it as the IdmEmulator `runtimeScripts` option (README: \"IDM runtime scripts\"); the fetch command `aic idm runtime-scripts pull` is planned and not yet shipped, so until it ships copy the files by hand in that layout";

/**
 * The path IDM's module loader looks up for `require(id)`: the id with `.js`
 * appended, under the script root. Rhino's loader tries `<id>.js` first; an id
 * that already ends in `.js` is taken as the file name (stock scripts call
 * both `require('lib/lodash')` and `require("lib/lodash.js")`). The two
 * spellings are still two module instances (the engine caches by id).
 */
export function modulePath(id: string): string {
  return id.endsWith(".js") ? id : `${id}.js`;
}

type Entry = "file" | "directory" | "symlink" | "other";
type Lookup = { text: string } | { absent: string } | { refused: string };

/** Why a name is not a canonical path under the script root, or undefined when it is. */
function nonCanonical(path: string): string | undefined {
  if (path.length === 0) return "it is empty";
  if (path.includes("\0")) return "it contains a NUL byte";
  if (path.includes("\\")) return "it contains a backslash";
  if (isAbsolute(path) || path.startsWith("/")) return "it is absolute";
  if (path.endsWith("/")) return "it ends with a slash";
  const bad = path.split("/").find((segment) => segment === "" || segment === "." || segment === "..");
  if (bad !== undefined) return bad === "" ? "it has an empty segment" : `it has a ${JSON.stringify(bad)} segment`;
  return undefined;
}

/**
 * Every entry under `root`, keyed by its posix path relative to it. Symbolic
 * links are recorded and never followed, so nothing outside the root is
 * reachable and a link inside it cannot stand in for a tenant file.
 */
function index(root: string): Map<string, Entry> {
  const entries = new Map<string, Entry>();
  const walk = (dir: string, prefix: string) => {
    for (const dirent of readdirSync(dir, { withFileTypes: true })) {
      const key = prefix + dirent.name;
      if (dirent.isSymbolicLink()) entries.set(key, "symlink");
      else if (dirent.isDirectory()) { entries.set(key, "directory"); walk(join(dir, dirent.name), `${key}/`); }
      else entries.set(key, dirent.isFile() ? "file" : "other");
    }
  };
  try {
    walk(root, "");
  } catch (error) {
    // An unreadable subdirectory (EACCES) is a configuration error like a missing root, so it
    // gets the same TypeError shape rather than a raw fs error from the constructor.
    throw new TypeError(`Invalid IDM runtimeScripts: cannot index ${root}: ${(error as Error).message}`);
  }
  return entries;
}

export class RuntimeScripts {
  readonly #provider: RuntimeScriptProvider | undefined;
  /** The directory provider's resolved root, its entries and a lower-cased lookup for case hints. */
  readonly #root: string | undefined;
  /** `#root` with its links resolved, which a read's realpath must sit under (P3-A). */
  readonly #realRoot: string | undefined;
  readonly #entries = new Map<string, Entry>();
  readonly #folded = new Map<string, string>();
  readonly #read = new Map<string, string>();

  constructor(provider: RuntimeScriptProvider | undefined) {
    if (provider !== undefined && typeof provider !== "string" && typeof provider !== "function" && !(provider instanceof URL)) {
      throw new TypeError("Invalid IDM runtimeScripts: expected a directory (absolute path or file: URL) or a loader function");
    }
    this.#provider = provider;
    if (typeof provider === "function" || provider === undefined) return;
    if (typeof provider === "string" && provider.length === 0) {
      throw new TypeError("Invalid IDM runtimeScripts: directory path is empty");
    }
    if (provider instanceof URL && provider.protocol !== "file:") {
      throw new TypeError(`Invalid IDM runtimeScripts: directory URL ${provider.href} is not a file: URL`);
    }
    if (typeof provider === "string" && !isAbsolute(provider)) {
      // A relative path would resolve against process.cwd(), which differs between running
      // Vitest at the repository root and in a package; the base has to be explicit.
      throw new TypeError(`Invalid IDM runtimeScripts: directory ${JSON.stringify(provider)} is relative; pass an absolute path or a file: URL such as new URL("./idm-runtime-scripts/", import.meta.url)`);
    }
    const root = typeof provider === "string" ? provider : fileURLToPath(provider);
    let directory: boolean;
    try { directory = statSync(root).isDirectory(); } catch { directory = false; }
    if (!directory) throw new TypeError(`Invalid IDM runtimeScripts: ${root} is not a directory`);
    this.#root = root;
    this.#realRoot = realpathSync(root);
    for (const [key, entry] of index(root)) {
      this.#entries.set(key, entry);
      this.#folded.set(key.toLowerCase(), key);
    }
  }

  get provider(): RuntimeScriptProvider | undefined { return this.#provider; }

  /**
   * The source of IDM runtime script `path`, or the harness fault naming it.
   * `reach` says how the script was reached, for the message.
   */
  async source(path: string, reach: string, evidence: Evidence): Promise<string> {
    const why = nonCanonical(path);
    if (why !== undefined) {
      // Rhino resolves bare ids under the script root and refuses one that leaves it;
      // its exact refusal is unmeasured, so a non-canonical name is not guessed at.
      return missing(evidence, `IDM runtime script name ${JSON.stringify(path)} (${reach}) is not a canonical path under the script root: ${why}`, RUNTIME_SCRIPT_PATH_FAULT);
    }
    const found = await this.#load(path, reach, evidence);
    if ("refused" in found) {
      return missing(evidence, `IDM runtime script ${path} (${reach}): ${found.refused}`, RUNTIME_SCRIPT_PATH_FAULT);
    }
    if ("absent" in found) {
      return missing(evidence, `IDM runs its runtime script ${path} here (${reach}), and ${found.absent}; ${RUNTIME_SCRIPT_FETCH}`, `${RUNTIME_SCRIPT_FAULT}/${path}`);
    }
    evidence.assumed("IDM runtime scripts come from the project's provider, as fetched from the tenant; their currency is the project's");
    return found.text;
  }

  async #load(path: string, reach: string, evidence: Evidence): Promise<Lookup> {
    const provider = this.#provider;
    if (provider === undefined) return { absent: "no runtime-script provider is configured" };
    if (typeof provider === "function") {
      let text: unknown;
      try {
        text = await provider(path);
      } catch (error) {
        if (emulationFailure(error)) throw error;
        return missing(evidence, `the runtimeScripts loader threw for ${path} (${reach}): ${error instanceof Error ? error.message : String(error)}`, RUNTIME_SCRIPT_PROVIDER_FAULT);
      }
      if (text === undefined) return { absent: "the provider function returned undefined" };
      if (typeof text !== "string") {
        return missing(evidence, `the runtimeScripts loader returned ${text === null ? "null" : typeof text} for ${path} (${reach}); expected a string or undefined`, RUNTIME_SCRIPT_PROVIDER_FAULT);
      }
      return { text };
    }
    const root = this.#root as string;
    const cached = this.#read.get(path);
    if (cached !== undefined) return { text: cached };
    const segments = path.split("/");
    for (let end = 1; end < segments.length; end += 1) {
      const prefix = segments.slice(0, end).join("/");
      if (this.#entries.get(prefix) === "symlink") return { refused: `${prefix} in the provider directory ${root} is a symbolic link; the directory provider follows none` };
    }
    const entry = this.#entries.get(path);
    if (entry === "symlink") return { refused: `it is a symbolic link in the provider directory ${root}; the directory provider admits regular files only` };
    if (entry === "other") return { refused: `it is not a regular file in the provider directory ${root}` };
    if (entry === "directory") return { absent: `the provider directory ${root} has a directory, not a file, by that name` };
    if (entry === undefined) {
      const near = this.#folded.get(path.toLowerCase());
      const indexed = `the provider directory ${root} has no such file as indexed when the emulator was constructed (a file added afterwards is not seen)`;
      return { absent: near === undefined ? indexed : `${indexed}; it has ${near}, which differs in case, and AIC's lookup is case-sensitive` };
    }
    return this.#readIndexed(path, segments, reach, evidence);
  }

  /**
   * Read an indexed regular file without following a link that replaced it, or one of its
   * directories, after construction (review e15-02 P3-A). The open refuses a final-component
   * link (`O_NOFOLLOW`) and never blocks on a FIFO (`O_NONBLOCK`). The bytes come from that
   * descriptor, and it is accepted only if it is a regular file whose identity (dev, ino) is the
   * file now at the canonical path under the resolved root. A directory swapped for a link
   * between the open and that check moves the realpath or the identity, so it is refused too.
   */
  #readIndexed(path: string, segments: string[], reach: string, evidence: Evidence): Lookup {
    const root = this.#root as string;
    const full = join(root, ...segments);
    const linked = { refused: `it, or a directory on its path, was replaced by a symbolic link in the provider directory ${root} after the emulator was constructed; the directory provider follows none` };
    let fd: number;
    try {
      fd = openSync(full, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ELOOP") return linked;
      return missing(evidence, `reading ${path} from the provider directory ${root} (${reach}) failed: ${(error as Error).message}`, RUNTIME_SCRIPT_PROVIDER_FAULT);
    }
    try {
      const opened = fstatSync(fd);
      if (!opened.isFile()) return { refused: `it is no longer a regular file in the provider directory ${root}` };
      let here: { dev: number; ino: number } | undefined;
      try {
        if (realpathSync(full) === join(this.#realRoot as string, ...segments)) here = statSync(full);
      } catch { here = undefined; }
      if (here === undefined || here.dev !== opened.dev || here.ino !== opened.ino) return linked;
      let text: string;
      try {
        text = readFileSync(fd, "utf8");
      } catch (error) {
        return missing(evidence, `reading ${path} from the provider directory ${root} (${reach}) failed: ${(error as Error).message}`, RUNTIME_SCRIPT_PROVIDER_FAULT);
      }
      this.#read.set(path, text);
      return { text };
    } finally {
      closeSync(fd);
    }
  }
}
