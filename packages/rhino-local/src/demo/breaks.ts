/**
 * Breaks: faults injected into live demo traffic, read from kikori-aic's own
 * break file so one file drives both the generator and the emulator
 * (`kikori-aic/src/generate/breaks.rs` is the format's definition).
 *
 *   [{"from": "...", "to": "...", "kind": "endpoint",   "path": "/x", "share": 0.4, "slower": 3},
 *    {"from": "...", "to": "...", "kind": "node-fails", "node": "<node id>", "share": 0.5},
 *    {"from": "...", "to": "...", "kind": "template",   "stream": "am-core", "text": "failed for <*>", "per_hour": 200},
 *    {"from": "...", "to": "...", "kind": "surge",      "network": "203.0.113.0/24", "per_hour": 600}]
 *
 * Same refusals as the Rust parser: unknown fields, an empty window, a share
 * outside 0..1, a negative rate, a network that is not IPv4 CIDR. A window is
 * `[from, to)` in virtual time.
 *
 * Randomness: a break never draws from a stream the base traffic uses. A
 * per-request decision is a keyed hash of (seed, break, request), and a break
 * that makes records has its own arrival stream, so the records outside a
 * break's window are those of a run without it.
 */
import { parseCidr, type Cidr } from "./cidr.ts";
import { keyedUnit, Rng } from "./rng.ts";
import type { LogSource } from "../logs/record.ts";
import { parseRfc3339Ns } from "../logs/time.ts";

export type BreakKind =
  | { readonly kind: "endpoint"; readonly path: string; readonly share: number; readonly slower: number }
  | { readonly kind: "node-fails"; readonly node: string; readonly share: number }
  | { readonly kind: "template"; readonly stream: LogSource; readonly text: string; readonly perHour: number }
  | { readonly kind: "surge"; readonly network: Cidr; readonly perHour: number };

export type Break = BreakKind & {
  /** Position in the file; part of every random key, so reordering the file changes what each break draws. */
  readonly index: number;
  readonly from: bigint;
  readonly to: bigint;
};

const STREAMS: readonly string[] = [
  "am-access", "am-activity", "am-authentication", "am-config", "am-core",
  "idm-access", "idm-activity", "idm-authentication", "idm-config", "idm-core", "idm-sync",
];

const KINDS = {
  endpoint: ["path", "share", "slower"],
  "node-fails": ["node", "share"],
  template: ["stream", "text", "per_hour"],
  surge: ["network", "per_hour"],
} as const;

export class BreakError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "BreakError";
  }
}

/** Parses a break file's text. Throws `BreakError` naming the break. */
export function parseBreaks(text: string): Break[] {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new BreakError(`break file: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (!Array.isArray(value)) throw new BreakError("break file: must be a JSON array of breaks");
  return value.map((raw, index): Break => {
    const at = `break ${index}`;
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) throw new BreakError(`${at}: must be an object`);
    const rec = raw as Record<string, unknown>;
    const kind = rec["kind"];
    if (typeof kind !== "string" || !Object.hasOwn(KINDS, kind)) {
      throw new BreakError(`${at}: kind must be one of ${Object.keys(KINDS).join(", ")}`);
    }
    const allowed: readonly string[] = ["from", "to", "kind", ...KINDS[kind as keyof typeof KINDS]];
    for (const key of Object.keys(rec)) {
      if (!allowed.includes(key)) throw new BreakError(`${at}: unknown field ${JSON.stringify(key)} for kind ${kind}`);
    }
    const time = (key: string): bigint => {
      const v = rec[key];
      if (typeof v !== "string") throw new BreakError(`${at}: ${key} must be an RFC 3339 string`);
      try {
        return parseRfc3339Ns(v);
      } catch (error) {
        throw new BreakError(`${at}: ${key}: ${error instanceof Error ? error.message : String(error)}`);
      }
    };
    const from = time("from");
    const to = time("to");
    if (to <= from) throw new BreakError(`${at}: its window is empty`);
    const num = (key: string, lo: number, hi: number, fallback?: number): number => {
      const v = rec[key] ?? fallback;
      if (typeof v !== "number" || !Number.isFinite(v) || v < lo || v > hi) {
        throw new BreakError(`${at}: ${key} must be a number from ${lo} to ${hi}`);
      }
      return v;
    };
    const str = (key: string): string => {
      const v = rec[key];
      if (typeof v !== "string") throw new BreakError(`${at}: ${key} must be a string`);
      return v;
    };
    const base = { index, from, to };
    switch (kind) {
      case "endpoint":
        return { ...base, kind, path: str("path"), share: num("share", 0, 1), slower: num("slower", 0, 1e6, 1) };
      case "node-fails":
        return { ...base, kind, node: str("node"), share: num("share", 0, 1) };
      case "template": {
        const stream = str("stream");
        if (!STREAMS.includes(stream)) throw new BreakError(`${at}: stream ${JSON.stringify(stream)} is not a log stream`);
        return { ...base, kind, stream: stream as LogSource, text: str("text"), perHour: num("per_hour", 0, 1e9) };
      }
      default: {
        const cidr = parseCidr(str("network"));
        if (cidr === undefined) throw new BreakError(`${at}: network is not IPv4 CIDR`);
        return { ...base, kind: "surge", network: cidr, perHour: num("per_hour", 0, 1e9) };
      }
    }
  });
}

export const inWindow = (b: Break, t: bigint): boolean => t >= b.from && t < b.to;

/** What the endpoint breaks do to one request, or undefined when none touch it. */
export type EndpointEffect = { readonly slower: number };

/**
 * Whether request `key` at time `t` to `path` fails under an endpoint break.
 * `path` is matched whole and also by its path part, so a break written
 * against `/am/...` hits a record that carries `https://host/am/...`.
 */
export function endpointEffect(
  breaks: readonly Break[],
  seed: string | number,
  t: bigint,
  path: string,
  key: string,
): EndpointEffect | undefined {
  let hit: EndpointEffect | undefined;
  const pathname = path.replace(/^https?:\/\/[^/]+/, "");
  for (const b of breaks) {
    if (b.kind !== "endpoint" || !inWindow(b, t)) continue;
    if (!path.startsWith(b.path) && !pathname.startsWith(b.path)) continue;
    if (keyedUnit(seed, "break", b.index, key) < b.share) {
      hit = { slower: Math.max(hit?.slower ?? 0, b.slower) };
    }
  }
  return hit;
}

/** Whether a journey reaching node `node` at `t` is made to fail there. `key` names the journey. */
export function nodeFails(breaks: readonly Break[], seed: string | number, t: bigint, node: string, key: string): boolean {
  return breaks.some(
    (b) => b.kind === "node-fails" && b.node === node && inWindow(b, t) && keyedUnit(seed, "break", b.index, key) < b.share,
  );
}

const LOGGERS: Record<string, readonly string[]> = {
  "am-core": [
    "org.forgerock.openam.core.rest.authn.RestAuthenticationHandler",
    "org.forgerock.openam.auth.trees.engine.AuthTreeExecutor",
    "org.forgerock.openam.scripting.service.ScriptEvaluator",
  ],
  "idm-core": [
    "org.forgerock.openidm.servlet.internal.ServletComponent",
    "org.forgerock.openidm.repo.opendj.impl.OpenDJRepoService",
  ],
};

/** The message with each `<*>` filled by a plausible varying token. */
export function fill(text: string, rng: Rng): string {
  return text.replaceAll("<*>", () => {
    switch (rng.below(3)) {
      case 0:
        return rng.hex(8);
      case 1:
        return String(rng.below(100_000));
      default:
        return rng.uuid();
    }
  });
}

/** Level and logger for a template record on `stream`: invented, plausible, fixed per break. */
export function templateVoice(b: Break & { kind: "template" }): { level: string | undefined; logger: string | undefined } {
  const loggers = LOGGERS[b.stream];
  const rng = Rng.derive("template-voice", b.index, b.text);
  return {
    level: b.stream === "am-core" ? "ERROR" : b.stream === "idm-core" ? "SEVERE" : undefined,
    logger: loggers === undefined ? undefined : rng.pick(loggers),
  };
}
