/**
 * RFC 3339 timestamps as integer nanoseconds since the Unix epoch.
 *
 * The tenant stamps records at nanosecond precision, and a bound compared as a
 * JS `Date` or a double would round two records a microsecond apart to the same
 * instant. Everything in the log store is therefore a `bigint` of nanoseconds.
 */

const RFC3339 =
  /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?([Zz]|[+-]\d{2}:\d{2})$/;

const NS_PER_SECOND = 1_000_000_000n;

function daysFromCivil(year: number, month: number, day: number): bigint {
  const y = month <= 2 ? year - 1 : year;
  const era = Math.floor(y / 400);
  const yoe = y - era * 400;
  const mp = (month + 9) % 12;
  const doy = Math.floor((153 * mp + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return BigInt(era * 146097 + doe - 719468);
}

function daysInMonth(year: number, month: number): number {
  if (month === 2) {
    return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0 ? 29 : 28;
  }
  return [4, 6, 9, 11].includes(month) ? 30 : 31;
}

/** Parses an RFC 3339 timestamp (any offset, up to nine fractional digits). Throws on anything else. */
export function parseRfc3339Ns(text: string): bigint {
  const m = RFC3339.exec(text);
  if (m === null) {
    throw new Error(`not an RFC 3339 timestamp: ${JSON.stringify(text)}`);
  }
  const [, ys, mos, ds, hs, mis, ss, frac, zone] = m as unknown as [
    string, string, string, string, string, string, string, string | undefined, string,
  ];
  const year = Number(ys);
  const month = Number(mos);
  const day = Number(ds);
  const hour = Number(hs);
  const minute = Number(mis);
  const second = Number(ss);
  if (
    month < 1 ||
    month > 12 ||
    day < 1 ||
    day > daysInMonth(year, month) ||
    hour > 23 ||
    minute > 59 ||
    second > 59
  ) {
    throw new Error(`not an RFC 3339 timestamp: ${JSON.stringify(text)}`);
  }
  let offsetSeconds = 0;
  if (zone !== "Z" && zone !== "z") {
    const oh = Number(zone.slice(1, 3));
    const om = Number(zone.slice(4, 6));
    if (oh > 23 || om > 59) {
      throw new Error(`not an RFC 3339 timestamp: ${JSON.stringify(text)}`);
    }
    offsetSeconds = (oh * 3600 + om * 60) * (zone.startsWith("-") ? -1 : 1);
  }
  const fraction = BigInt((frac ?? "").padEnd(9, "0") || "0");
  const seconds =
    daysFromCivil(year, month, day) * 86400n +
    BigInt(hour * 3600 + minute * 60 + second - offsetSeconds);
  return seconds * NS_PER_SECOND + fraction;
}

/** Formats nanoseconds as RFC 3339 in UTC, trimming trailing zeros of the fraction. */
export function formatRfc3339Ns(ns: bigint): string {
  let seconds = ns / NS_PER_SECOND;
  let fraction = ns % NS_PER_SECOND;
  if (fraction < 0n) {
    fraction += NS_PER_SECOND;
    seconds -= 1n;
  }
  const date = new Date(Number(seconds) * 1000).toISOString().slice(0, 19);
  const digits = fraction.toString().padStart(9, "0").replace(/0+$/, "");
  return `${date}${digits === "" ? "" : `.${digits}`}Z`;
}
