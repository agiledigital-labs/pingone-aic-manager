/** Out-of-band numeric paths avoid collisions with literal JSON object fields. */
type Numeric = { path: string[]; kind: "NaN" | "Infinity" | "-Infinity" | "-0" };
export function stringifyProtocol(record: object): string {
  const numbers: Numeric[] = [];
  const visit = (value: unknown, path: string[]): unknown => {
    if (typeof value === "number" && (!Number.isFinite(value) || Object.is(value, -0))) {
      numbers.push({ path, kind: Number.isNaN(value) ? "NaN" : Object.is(value, -0) ? "-0" : value > 0 ? "Infinity" : "-Infinity" });
      return null;
    }
    if (Array.isArray(value)) return value.map((child, index) => visit(child, [...path, String(index)]));
    if (value !== null && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, visit(child, [...path, key])]));
    return value;
  };
  const encoded = visit(record, []) as Record<string, unknown>;
  if (numbers.length) encoded._rhinoNumbers = numbers;
  return JSON.stringify(encoded);
}

export function decodeProtocol(value: unknown): unknown {
  if (value === null || typeof value !== "object" || !Object.hasOwn(value, "_rhinoNumbers")) return value;
  const record = value as Record<string, unknown>;
  const numbers = record._rhinoNumbers as Numeric[];
  delete record._rhinoNumbers;
  for (const number of numbers) {
    let target: Record<string, unknown> = record;
    for (const key of number.path.slice(0, -1)) {
      if (!Object.hasOwn(target, key) || target[key] === null || typeof target[key] !== "object") throw new Error("invalid protocol numeric path");
      target = target[key] as Record<string, unknown>;
    }
    const key = number.path.at(-1);
    if (key === undefined || !Object.hasOwn(target, key) || !["NaN", "Infinity", "-Infinity", "-0"].includes(number.kind)) throw new Error("invalid protocol numeric value");
    Object.defineProperty(target, key, { value: Number(number.kind), enumerable: true, configurable: true, writable: true });
  }
  return record;
}
