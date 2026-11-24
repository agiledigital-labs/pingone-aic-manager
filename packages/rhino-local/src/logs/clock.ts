/**
 * The shared clock. Mirrors Kikori's `kikori-event/src/clock.rs`: when
 * `KIKORI_CLOCK` names a JSON file `{real_origin, virtual_origin, rate}`, now is
 * `virtual_origin + (real now - real_origin) * rate`, read once at start; with
 * no variable it is real time. Arithmetic is exact integer nanoseconds (the
 * rate is decomposed into its binary mantissa and exponent), truncating toward
 * zero as Kikori's does.
 */
import { readFileSync } from "node:fs";
import { parseRfc3339Ns } from "./time.ts";

/** The environment variable naming the clock file. */
export const CLOCK_ENV = "KIKORI_CLOCK";

export interface Clock {
  /** Now, in nanoseconds since the epoch, on this clock. */
  nowNs(): bigint;
}

/** The source of real time, in nanoseconds since the epoch. Injectable for tests. */
export type RealNowNs = () => bigint;

export const systemNowNs: RealNowNs = () => BigInt(Date.now()) * 1_000_000n;

export type ClockSpec = {
  readonly realOriginNs: bigint;
  readonly virtualOriginNs: bigint;
  readonly rate: number;
};

/** Parses the clock file's text. Throws a `clock file: …` error naming what is wrong. */
export function parseClock(text: string): ClockSpec {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new Error(`clock file: ${error instanceof Error ? error.message : String(error)}`, {
      cause: error,
    });
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("clock file: must be a JSON object");
  }
  const record = value as Record<string, unknown>;
  const time = (key: string): bigint => {
    const raw = record[key];
    if (typeof raw !== "string") {
      throw new Error(`clock file: ${key} must be an RFC 3339 string`);
    }
    try {
      return parseRfc3339Ns(raw);
    } catch (error) {
      throw new Error(`clock file: ${key}: ${error instanceof Error ? error.message : String(error)}`, {
        cause: error,
      });
    }
  };
  const rate = record["rate"];
  if (typeof rate !== "number" || !Number.isFinite(rate) || rate <= 0) {
    throw new Error("clock file: rate must be a positive number");
  }
  return { realOriginNs: time("real_origin"), virtualOriginNs: time("virtual_origin"), rate };
}

/** `m * 2^e` exactly equal to the positive finite double `rate`. */
function decompose(rate: number): { m: bigint; e: number } {
  const view = new DataView(new ArrayBuffer(8));
  view.setFloat64(0, rate);
  const bits = view.getBigUint64(0);
  const exponent = Number((bits >> 52n) & 0x7ffn);
  const mantissa = bits & ((1n << 52n) - 1n);
  return exponent === 0
    ? { m: mantissa, e: -1074 }
    : { m: mantissa | (1n << 52n), e: exponent - 1075 };
}

/** The virtual time at real time `realNs`. */
export function virtualAt(spec: ClockSpec, realNs: bigint): bigint {
  const { m, e } = decompose(spec.rate);
  const product = (realNs - spec.realOriginNs) * m;
  const scaled = e >= 0 ? product << BigInt(e) : product / (1n << BigInt(-e));
  return spec.virtualOriginNs + scaled;
}

export function realClock(realNowNs: RealNowNs = systemNowNs): Clock {
  return { nowNs: realNowNs };
}

export function virtualClock(spec: ClockSpec, realNowNs: RealNowNs = systemNowNs): Clock {
  return { nowNs: () => virtualAt(spec, realNowNs()) };
}

/**
 * The process's clock: virtual when `KIKORI_CLOCK` names a file, real
 * otherwise. The file is read here, once; a missing or bad file throws.
 */
export function clockFromEnv(
  env: NodeJS.ProcessEnv = process.env,
  realNowNs: RealNowNs = systemNowNs,
  readFile: (path: string) => string = (path) => readFileSync(path, "utf8"),
): Clock {
  const path = env[CLOCK_ENV];
  if (path === undefined || path === "") {
    return realClock(realNowNs);
  }
  let text: string;
  try {
    text = readFile(path);
  } catch (error) {
    throw new Error(`${CLOCK_ENV}=${path}: ${error instanceof Error ? error.message : String(error)}`, {
      cause: error,
    });
  }
  return virtualClock(parseClock(text), realNowNs);
}
