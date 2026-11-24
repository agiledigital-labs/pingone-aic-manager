/** IPv4 networks in CIDR form, as a GeoIP database places them. */
import type { Rng } from "./rng.ts";

export type Cidr = { readonly base: number; readonly size: number; readonly text: string };

/** Parses `203.0.113.0/24`; host bits are masked off. Returns undefined when it is not IPv4 CIDR. */
export function parseCidr(text: string): Cidr | undefined {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\/(\d{1,2})$/.exec(text);
  if (m === null) return undefined;
  const octets = [m[1], m[2], m[3], m[4]].map(Number);
  const len = Number(m[5]);
  if (octets.some((o) => o > 255) || len > 32) return undefined;
  const addr = octets.reduce((acc, o) => acc * 256 + o, 0);
  const size = 2 ** (32 - len);
  const base = addr - (addr % size);
  return { base, size, text: `${dotted(base)}/${len}` };
}

export function dotted(addr: number): string {
  return [addr / 16777216, addr / 65536, addr / 256, addr].map((x) => Math.floor(x) % 256).join(".");
}

/** A random address in the network. */
export function address(cidr: Cidr, rng: Rng): string {
  return dotted(cidr.base + rng.below(cidr.size));
}
