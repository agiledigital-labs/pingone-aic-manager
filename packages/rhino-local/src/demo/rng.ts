/**
 * Seeded randomness for the demo driver. Every stream is derived from the run
 * seed and a list of labels, so a stream never depends on how many draws
 * another one made: that independence is what lets a break leave the records
 * outside its window unchanged.
 */
import { createHash } from "node:crypto";

export type Label = string | number | bigint;

export class Rng {
  #a: number;
  #b: number;
  #c: number;
  #d: number;

  private constructor(a: number, b: number, c: number, d: number) {
    this.#a = a;
    this.#b = b;
    this.#c = c;
    this.#d = d;
    for (let i = 0; i < 12; i++) this.#next();
  }

  /** The stream for `(seed, labels...)`. The same arguments always give the same stream. */
  static derive(seed: Label, ...labels: Label[]): Rng {
    const h = createHash("sha256")
      .update(JSON.stringify([String(seed), ...labels.map(String)]))
      .digest();
    return new Rng(h.readUInt32LE(0), h.readUInt32LE(4), h.readUInt32LE(8), h.readUInt32LE(12));
  }

  /** sfc32. */
  #next(): number {
    let t = (this.#a + this.#b) | 0;
    this.#a = this.#b ^ (this.#b >>> 9);
    this.#b = (this.#c + (this.#c << 3)) | 0;
    this.#c = (this.#c << 21) | (this.#c >>> 11);
    this.#d = (this.#d + 1) | 0;
    t = (t + this.#d) | 0;
    this.#c = (this.#c + t) | 0;
    return t >>> 0;
  }

  /** Uniform in [0, 1), 53 bits. */
  unit(): number {
    const hi = this.#next() >>> 5;
    const lo = this.#next() >>> 6;
    return (hi * 67108864 + lo) / 9007199254740992;
  }

  /** Uniform integer in [0, n). */
  below(n: number): number {
    return Math.min(n - 1, Math.floor(this.unit() * n));
  }

  between(lo: number, hi: number): number {
    return lo + this.unit() * (hi - lo);
  }

  /** Exponential with the given rate (events per unit). */
  exponential(rate: number): number {
    return -Math.log(1 - this.unit()) / rate;
  }

  normal(): number {
    const u = 1 - this.unit();
    const v = this.unit();
    return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
  }

  /** Log-normal with the given median and 95th percentile. */
  lognormal(median: number, p95: number): number {
    const mu = Math.log(median);
    const sigma = Math.max(0, Math.log(Math.max(p95, median)) - mu) / 1.6448536269514722;
    return Math.exp(mu + sigma * this.normal());
  }

  /** Index chosen with chance proportional to `shares` (non-negative, not all zero). */
  weighted(shares: readonly number[]): number {
    let total = 0;
    for (const s of shares) total += s;
    let x = this.unit() * total;
    for (const [i, s] of shares.entries()) {
      x -= s;
      if (x < 0) return i;
    }
    return shares.length - 1;
  }

  pick<T>(items: readonly T[]): T {
    return items[this.below(items.length)] as T;
  }

  hex(n: number): string {
    let out = "";
    while (out.length < n) out += (this.#next() >>> 0).toString(16).padStart(8, "0");
    return out.slice(0, n);
  }

  uuid(): string {
    const h = this.hex(32);
    return `${h.slice(0, 8)}-${h.slice(8, 12)}-4${h.slice(13, 16)}-8${h.slice(17, 20)}-${h.slice(20, 32)}`;
  }
}

/** One uniform draw keyed by `(seed, labels...)`, independent of every other draw. */
export function keyedUnit(seed: Label, ...labels: Label[]): number {
  return Rng.derive(seed, ...labels).unit();
}
