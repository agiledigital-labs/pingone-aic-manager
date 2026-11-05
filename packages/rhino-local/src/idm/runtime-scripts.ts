/**
 * IDM's own runtime scripts (E15): the files under the tenant's
 * `/opt/openidm/bin/defaults/script` (and the engine's bundled `lib/`), which
 * IDM runs from a hook's `file` reference or a `require(id)`. Nothing here
 * ships them. The project supplies a provider — a directory, normally one
 * fetched from the tenant and committed, or a loader function — and a script
 * the provider does not supply is the harness fault `idm-runtime-script/<path>`.
 */
import { readFileSync } from "node:fs";
import { isAbsolute, join, posix } from "node:path";
import { missing } from "./errors.ts";
import type { Evidence } from "./provenance.ts";

/**
 * A directory whose files are named by IDM's relative script path
 * (`<dir>/roles/effectiveRoles.js`), or a loader answering that path with the
 * script's source, or `undefined` when it does not have it.
 */
export type RuntimeScriptProvider =
  | string
  | ((scriptPath: string) => string | undefined | Promise<string | undefined>);

/** The fault-id family; the id is `idm-runtime-script/<path>`. */
export const RUNTIME_SCRIPT_FAULT = "idm-runtime-script";
/** A `require` id the provider cannot be asked about, e.g. one that leaves the script root. */
export const RUNTIME_SCRIPT_PATH_FAULT = "idm-runtime-script-path";

/**
 * How to get the scripts, named in every refusal. The `aic` command is the
 * planned fetch (E15 follow-on slice); change it here when that ships.
 */
export const RUNTIME_SCRIPT_FETCH =
  "fetch the tenant's IDM runtime scripts with `aic idm runtime-scripts pull` into a directory committed beside the config, and pass it as the IdmEmulator `runtimeScripts` option (README: \"IDM runtime scripts\")";

/**
 * The path IDM's module loader looks up for `require(id)`: the id with `.js`
 * appended, under the script root. Rhino's loader tries `<id>.js` first; an id
 * that already ends in `.js` is taken as the file name (stock scripts call
 * both `require('lib/lodash')` and `require("lib/lodash.js")`).
 */
export function modulePath(id: string): string {
  return id.endsWith(".js") ? id : `${id}.js`;
}

export class RuntimeScripts {
  readonly #provider: RuntimeScriptProvider | undefined;
  readonly #read = new Map<string, string | undefined>();

  constructor(provider: RuntimeScriptProvider | undefined) {
    if (provider !== undefined && typeof provider !== "string" && typeof provider !== "function") {
      throw new TypeError("Invalid IDM runtimeScripts: expected a directory path or a loader function");
    }
    if (typeof provider === "string" && provider.length === 0) {
      throw new TypeError("Invalid IDM runtimeScripts: directory path is empty");
    }
    this.#provider = provider;
  }

  get provider(): RuntimeScriptProvider | undefined { return this.#provider; }

  /**
   * The source of IDM runtime script `path`, or the harness fault naming it.
   * `reach` says how the script was reached, for the message.
   */
  async source(path: string, reach: string, evidence: Evidence): Promise<string> {
    const normal = posix.normalize(path);
    if (path.length === 0 || isAbsolute(path) || path.includes("\\") || normal.startsWith("../") || normal === ".." || normal !== path) {
      // Rhino resolves bare ids under the script root and refuses one that leaves it;
      // its exact refusal is unmeasured, so a non-canonical name is not guessed at.
      return missing(evidence, `IDM runtime script name ${JSON.stringify(path)} (${reach}) is not a canonical path under the script root`, RUNTIME_SCRIPT_PATH_FAULT);
    }
    const text = await this.#load(path);
    if (text === undefined) {
      const where = this.#provider === undefined ? "no runtime-script provider is configured" :
        typeof this.#provider === "string" ? `the provider directory ${this.#provider} has no such file` : "the provider function returned undefined";
      return missing(evidence, `IDM runs its runtime script ${path} here (${reach}), and ${where}; ${RUNTIME_SCRIPT_FETCH}`, `${RUNTIME_SCRIPT_FAULT}/${path}`);
    }
    evidence.assumed("IDM runtime scripts come from the project's provider, as fetched from the tenant; their currency is the project's");
    return text;
  }

  async #load(path: string): Promise<string | undefined> {
    const provider = this.#provider;
    if (provider === undefined) return undefined;
    if (typeof provider === "function") {
      const text = await provider(path);
      if (text !== undefined && typeof text !== "string") {
        throw new TypeError(`IDM runtimeScripts loader returned ${typeof text} for ${path}; expected a string or undefined`);
      }
      return text;
    }
    if (this.#read.has(path)) return this.#read.get(path);
    let text: string | undefined;
    try {
      text = readFileSync(join(provider, ...path.split("/")), "utf8");
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code !== "ENOENT" && code !== "ENOTDIR") throw error;
      text = undefined;
    }
    this.#read.set(path, text);
    return text;
  }
}
