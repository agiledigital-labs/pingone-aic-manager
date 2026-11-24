/**
 * The traffic-mix file: what the demo driver runs. JSON, validated here, with
 * every refusal named by its path in the file. Nothing in it is tenant
 * specific; the journeys it names are whatever the journeys file defines.
 *
 * See README "Demo driver" for the format with an example.
 */
import { parseCidr, type Cidr } from "./cidr.ts";

export class MixError extends Error {
  readonly problems: readonly string[];
  constructor(problems: readonly string[]) {
    super(`invalid traffic mix:\n${problems.map((p) => `  - ${p}`).join("\n")}`);
    this.name = "MixError";
    this.problems = problems;
  }
}

/** A rate, per virtual hour, as a function of the hour of the week (Monday 00:00 UTC is hour 0). */
export type RateCurve =
  | { readonly kind: "curves"; readonly perHour: number; readonly daily: readonly number[]; readonly weekly: readonly number[] }
  | { readonly kind: "hours"; readonly byHourOfWeek: readonly number[] };

export type Latency = { readonly medianMs: number; readonly p95Ms: number };
export type Network = { readonly cidr: Cidr; readonly share: number };
export type Users = { readonly count: number; readonly domain: string };

export type Answer = {
  readonly label: string;
  /** What the author intends this answer to lead to; the driver tallies by it. */
  readonly outcome: "success" | "failure" | "error";
  readonly share: number;
  /** Callback type to the value put in its first input. `{username}`, `{email}` and `{userId}` are filled in. */
  readonly fill: Readonly<Record<string, string>>;
};

export type Step = {
  readonly thinkSeconds: readonly [number, number];
  readonly answers: readonly Answer[];
};

export type HttpResponseSpec = {
  readonly share: number;
  readonly status: number;
  readonly body?: unknown;
};
export type HttpRule = { readonly match: RegExp; readonly source: string; readonly responses: readonly HttpResponseSpec[] };

export type JourneySpec = {
  readonly tree: string;
  readonly rate: RateCurve;
  readonly users: Users;
  readonly networks: readonly Network[];
  readonly userAgents: readonly string[];
  readonly latency: Latency;
  readonly steps: readonly Step[];
  /** Share of journeys that stop part way and never return. */
  readonly abandon: number;
  readonly http: readonly HttpRule[];
};

export type StatusSpec = {
  readonly share: number;
  readonly status: "SUCCESSFUL" | "FAILED";
  readonly statusCode: number;
};

export type CoreLine = {
  readonly stream: "am-core" | "idm-core";
  readonly chance: number;
  readonly level: string;
  readonly logger: string;
  readonly message: string;
};

export type RequestSpec = {
  readonly name: string;
  readonly stream: "am-access" | "idm-access";
  readonly method: string;
  /** May contain `{userId}`, filled from the request's user. */
  readonly path: string;
  readonly query: Readonly<Record<string, string>>;
  readonly component: string;
  readonly rate: RateCurve;
  readonly users: Users;
  readonly networks: readonly Network[];
  readonly userAgents: readonly string[];
  readonly latency: Latency;
  readonly status: readonly StatusSpec[];
  readonly core: readonly CoreLine[];
};

export type Mix = {
  readonly tenant: { readonly host: string | undefined; readonly realm: string | undefined };
  readonly journeys: Readonly<Record<string, JourneySpec>>;
  readonly requests: readonly RequestSpec[];
};

const DEFAULT_LATENCY: Latency = { medianMs: 40, p95Ms: 160 };
const DEFAULT_AGENT = "Mozilla/5.0 (X11; Linux x86_64) demo-client/1.0";
const SYNTHETIC_DOMAIN = /^(?:[a-z0-9-]+\.)*(?:example\.(?:com|net|org)|[a-z0-9-]+\.(?:example|test|invalid))$/;
const CORE_STREAMS = ["am-core", "idm-core"];

class Check {
  readonly problems: string[] = [];
  bad(path: string, message: string): void {
    this.problems.push(`${path}: ${message}`);
  }
  object(value: unknown, path: string, allowed: readonly string[]): Record<string, unknown> | undefined {
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      this.bad(path, "must be an object");
      return undefined;
    }
    const rec = value as Record<string, unknown>;
    for (const key of Object.keys(rec)) {
      if (!allowed.includes(key)) this.bad(`${path}.${key}`, `unknown field (allowed: ${allowed.join(", ")})`);
    }
    return rec;
  }
  array(value: unknown, path: string, min = 0): unknown[] | undefined {
    if (!Array.isArray(value)) {
      this.bad(path, "must be an array");
      return undefined;
    }
    if (value.length < min) this.bad(path, `must have at least ${min} item${min === 1 ? "" : "s"}`);
    return value;
  }
  number(value: unknown, path: string, lo: number, hi: number): number | undefined {
    if (typeof value !== "number" || !Number.isFinite(value) || value < lo || value > hi) {
      this.bad(path, `must be a number from ${lo} to ${hi}`);
      return undefined;
    }
    return value;
  }
  string(value: unknown, path: string): string | undefined {
    if (typeof value !== "string" || value === "") {
      this.bad(path, "must be a non-empty string");
      return undefined;
    }
    return value;
  }
}

function rate(c: Check, value: unknown, path: string): RateCurve | undefined {
  const rec = c.object(value, path, ["perHour", "daily", "weekly", "byHourOfWeek"]);
  if (rec === undefined) return undefined;
  const list = (v: unknown, p: string, n: number): number[] | undefined => {
    const arr = c.array(v, p);
    if (arr === undefined) return undefined;
    if (arr.length !== n) {
      c.bad(p, `must have exactly ${n} numbers, has ${arr.length}`);
      return undefined;
    }
    const out = arr.map((x, i) => c.number(x, `${p}[${i}]`, 0, 1e9));
    return out.every((x) => x !== undefined) ? (out as number[]) : undefined;
  };
  if (rec["byHourOfWeek"] !== undefined) {
    for (const k of ["perHour", "daily", "weekly"]) {
      if (rec[k] !== undefined) c.bad(`${path}.${k}`, "cannot be combined with byHourOfWeek");
    }
    const hours = list(rec["byHourOfWeek"], `${path}.byHourOfWeek`, 168);
    return hours === undefined ? undefined : { kind: "hours", byHourOfWeek: hours };
  }
  const perHour = c.number(rec["perHour"], `${path}.perHour`, 0, 1e9);
  const daily = rec["daily"] === undefined ? Array<number>(24).fill(1) : list(rec["daily"], `${path}.daily`, 24);
  const weekly = rec["weekly"] === undefined ? Array<number>(7).fill(1) : list(rec["weekly"], `${path}.weekly`, 7);
  if (perHour === undefined || daily === undefined || weekly === undefined) return undefined;
  return { kind: "curves", perHour, daily, weekly };
}

function latency(c: Check, value: unknown, path: string): Latency {
  if (value === undefined) return DEFAULT_LATENCY;
  const rec = c.object(value, path, ["medianMs", "p95Ms"]);
  if (rec === undefined) return DEFAULT_LATENCY;
  const median = c.number(rec["medianMs"], `${path}.medianMs`, 0.001, 1e6);
  const p95 = c.number(rec["p95Ms"], `${path}.p95Ms`, 0.001, 1e6);
  if (median !== undefined && p95 !== undefined && p95 < median) c.bad(path, "p95Ms must not be below medianMs");
  return { medianMs: median ?? DEFAULT_LATENCY.medianMs, p95Ms: p95 ?? DEFAULT_LATENCY.p95Ms };
}

function shared(c: Check, items: readonly { share: number }[], path: string): void {
  const total = items.reduce((a, i) => a + i.share, 0);
  if (items.length > 0 && !(total > 0)) c.bad(path, "shares must not all be zero");
}

function users(c: Check, value: unknown, path: string): Users {
  const fallback: Users = { count: 1000, domain: "example.com" };
  if (value === undefined) return fallback;
  const rec = c.object(value, path, ["count", "domain"]);
  if (rec === undefined) return fallback;
  const count = c.number(rec["count"], `${path}.count`, 1, 1e7);
  const domain = rec["domain"] === undefined ? "example.com" : c.string(rec["domain"], `${path}.domain`);
  if (domain !== undefined && !SYNTHETIC_DOMAIN.test(domain)) {
    c.bad(`${path}.domain`, "synthetic people only: use example.com, example.net, example.org or a .example/.test/.invalid name");
  }
  return { count: count !== undefined ? Math.floor(count) : 1, domain: domain ?? "example.com" };
}

function networks(c: Check, value: unknown, path: string): Network[] {
  const arr = c.array(value, path, 1);
  if (arr === undefined) return [];
  const out: Network[] = [];
  for (const [i, item] of arr.entries()) {
    const p = `${path}[${i}]`;
    const rec = c.object(item, p, ["cidr", "share"]);
    if (rec === undefined) continue;
    const text = c.string(rec["cidr"], `${p}.cidr`);
    const share = c.number(rec["share"], `${p}.share`, 0, 1e9);
    const cidr = text === undefined ? undefined : parseCidr(text);
    if (text !== undefined && cidr === undefined) c.bad(`${p}.cidr`, `${JSON.stringify(text)} is not an IPv4 CIDR such as 203.0.113.0/24`);
    if (cidr !== undefined && share !== undefined) out.push({ cidr, share });
  }
  shared(c, out, path);
  return out;
}

function agents(c: Check, value: unknown, path: string): string[] {
  if (value === undefined) return [DEFAULT_AGENT];
  const arr = c.array(value, path, 1);
  if (arr === undefined) return [DEFAULT_AGENT];
  const out = arr.map((a, i) => c.string(a, `${path}[${i}]`)).filter((a): a is string => a !== undefined);
  return out.length > 0 ? out : [DEFAULT_AGENT];
}

function journey(c: Check, tree: string, value: unknown, path: string): JourneySpec | undefined {
  const rec = c.object(value, path, ["rate", "users", "networks", "userAgents", "latencyMs", "steps", "abandon", "http"]);
  if (rec === undefined) return undefined;
  const r = rate(c, rec["rate"], `${path}.rate`);
  const stepsRaw = rec["steps"] === undefined ? [] : (c.array(rec["steps"], `${path}.steps`) ?? []);
  const steps: Step[] = [];
  for (const [i, raw] of stepsRaw.entries()) {
    const p = `${path}.steps[${i}]`;
    const s = c.object(raw, p, ["thinkSeconds", "answers"]);
    if (s === undefined) continue;
    let think: [number, number] = [2, 15];
    if (s["thinkSeconds"] !== undefined) {
      const t = c.array(s["thinkSeconds"], `${p}.thinkSeconds`);
      if (t !== undefined) {
        const lo = c.number(t[0], `${p}.thinkSeconds[0]`, 0, 86400);
        const hi = c.number(t[1], `${p}.thinkSeconds[1]`, 0, 86400);
        if (t.length !== 2) c.bad(`${p}.thinkSeconds`, "must be [min, max]");
        else if (lo !== undefined && hi !== undefined) {
          if (hi < lo) c.bad(`${p}.thinkSeconds`, "max is below min");
          think = [lo, hi];
        }
      }
    }
    const answersRaw = c.array(s["answers"], `${p}.answers`, 1) ?? [];
    const answers: Answer[] = [];
    for (const [j, a] of answersRaw.entries()) {
      const ap = `${p}.answers[${j}]`;
      const ar = c.object(a, ap, ["label", "outcome", "share", "fill"]);
      if (ar === undefined) continue;
      const share = c.number(ar["share"], `${ap}.share`, 0, 1e9);
      const outcome = ar["outcome"];
      if (outcome !== "success" && outcome !== "failure" && outcome !== "error") {
        c.bad(`${ap}.outcome`, 'must be "success", "failure" or "error"');
      }
      const fillRec = c.object(ar["fill"], `${ap}.fill`, Object.keys((ar["fill"] ?? {}) as object));
      const fill: Record<string, string> = {};
      for (const [k, v] of Object.entries(fillRec ?? {})) {
        if (typeof v !== "string") c.bad(`${ap}.fill.${k}`, "must be a string");
        else fill[k] = v;
      }
      const label = ar["label"] === undefined ? `answer ${j}` : c.string(ar["label"], `${ap}.label`);
      if (share !== undefined && (outcome === "success" || outcome === "failure" || outcome === "error") && label !== undefined) {
        answers.push({ label, outcome, share, fill });
      }
    }
    shared(c, answers, `${p}.answers`);
    steps.push({ thinkSeconds: think, answers });
  }
  const abandon = rec["abandon"] === undefined ? 0 : c.number(rec["abandon"], `${path}.abandon`, 0, 1);
  if ((abandon ?? 0) > 0 && steps.length === 0) c.bad(`${path}.abandon`, "a journey with no steps never stops part way; give it steps or set abandon to 0");
  const httpRaw = rec["http"] === undefined ? [] : (c.array(rec["http"], `${path}.http`) ?? []);
  const http: HttpRule[] = [];
  for (const [i, raw] of httpRaw.entries()) {
    const p = `${path}.http[${i}]`;
    const h = c.object(raw, p, ["match", "responses"]);
    if (h === undefined) continue;
    const source = c.string(h["match"], `${p}.match`);
    let match: RegExp | undefined;
    if (source !== undefined) {
      try {
        match = new RegExp(source);
      } catch {
        c.bad(`${p}.match`, `${JSON.stringify(source)} is not a regular expression`);
      }
    }
    const respRaw = c.array(h["responses"], `${p}.responses`, 1) ?? [];
    const responses: HttpResponseSpec[] = [];
    for (const [j, rr] of respRaw.entries()) {
      const rp = `${p}.responses[${j}]`;
      const o = c.object(rr, rp, ["share", "status", "body"]);
      if (o === undefined) continue;
      const share = c.number(o["share"], `${rp}.share`, 0, 1e9);
      const status = c.number(o["status"], `${rp}.status`, 100, 599);
      if (share !== undefined && status !== undefined) {
        responses.push({
          share,
          status: Math.floor(status),
          ...(o["body"] === undefined ? {} : { body: o["body"] }),
        });
      }
    }
    shared(c, responses, `${p}.responses`);
    if (match !== undefined && source !== undefined) http.push({ match, source, responses });
  }
  if (r === undefined) return undefined;
  return {
    tree,
    rate: r,
    users: users(c, rec["users"], `${path}.users`),
    networks: networks(c, rec["networks"], `${path}.networks`),
    userAgents: agents(c, rec["userAgents"], `${path}.userAgents`),
    latency: latency(c, rec["latencyMs"], `${path}.latencyMs`),
    steps,
    abandon: abandon ?? 0,
    http,
  };
}

function request(c: Check, value: unknown, path: string): RequestSpec | undefined {
  const rec = c.object(value, path, [
    "name", "stream", "method", "path", "query", "component", "rate", "users", "networks", "userAgents", "latencyMs", "status", "core",
  ]);
  if (rec === undefined) return undefined;
  const name = c.string(rec["name"], `${path}.name`);
  const stream = rec["stream"];
  if (stream !== "am-access" && stream !== "idm-access") c.bad(`${path}.stream`, 'must be "am-access" or "idm-access"');
  const method = c.string(rec["method"], `${path}.method`);
  const p = c.string(rec["path"], `${path}.path`);
  if (p !== undefined && !p.startsWith("/")) c.bad(`${path}.path`, "must start with /");
  const query: Record<string, string> = {};
  if (rec["query"] !== undefined) {
    const q = c.object(rec["query"], `${path}.query`, Object.keys((rec["query"] ?? {}) as object));
    for (const [k, v] of Object.entries(q ?? {})) {
      if (typeof v !== "string") c.bad(`${path}.query.${k}`, "must be a string");
      else query[k] = v;
    }
  }
  const component = rec["component"] === undefined ? "Authentication" : c.string(rec["component"], `${path}.component`);
  const r = rate(c, rec["rate"], `${path}.rate`);
  const statusRaw = c.array(rec["status"], `${path}.status`, 1) ?? [];
  const status: StatusSpec[] = [];
  for (const [i, raw] of statusRaw.entries()) {
    const sp = `${path}.status[${i}]`;
    const s = c.object(raw, sp, ["share", "status", "statusCode"]);
    if (s === undefined) continue;
    const share = c.number(s["share"], `${sp}.share`, 0, 1e9);
    const st = s["status"];
    if (st !== "SUCCESSFUL" && st !== "FAILED") c.bad(`${sp}.status`, 'must be "SUCCESSFUL" or "FAILED"');
    const code = c.number(s["statusCode"], `${sp}.statusCode`, 100, 599);
    if (share !== undefined && code !== undefined && (st === "SUCCESSFUL" || st === "FAILED")) {
      status.push({ share, status: st, statusCode: Math.floor(code) });
    }
  }
  shared(c, status, `${path}.status`);
  const coreRaw = rec["core"] === undefined ? [] : (c.array(rec["core"], `${path}.core`) ?? []);
  const core: CoreLine[] = [];
  for (const [i, raw] of coreRaw.entries()) {
    const cp = `${path}.core[${i}]`;
    const o = c.object(raw, cp, ["stream", "chance", "level", "logger", "message"]);
    if (o === undefined) continue;
    const cs = o["stream"];
    if (typeof cs !== "string" || !CORE_STREAMS.includes(cs)) c.bad(`${cp}.stream`, 'must be "am-core" or "idm-core"');
    const chance = c.number(o["chance"], `${cp}.chance`, 0, 1);
    const level = c.string(o["level"], `${cp}.level`);
    const logger = c.string(o["logger"], `${cp}.logger`);
    const message = c.string(o["message"], `${cp}.message`);
    if ((cs === "am-core" || cs === "idm-core") && chance !== undefined && level !== undefined && logger !== undefined && message !== undefined) {
      core.push({ stream: cs, chance, level, logger, message });
    }
  }
  if (name === undefined || (stream !== "am-access" && stream !== "idm-access") || method === undefined || p === undefined || r === undefined) {
    return undefined;
  }
  return {
    name,
    stream,
    method,
    path: p,
    query,
    component: component ?? "Authentication",
    rate: r,
    users: users(c, rec["users"], `${path}.users`),
    networks: networks(c, rec["networks"], `${path}.networks`),
    userAgents: agents(c, rec["userAgents"], `${path}.userAgents`),
    latency: latency(c, rec["latencyMs"], `${path}.latencyMs`),
    status,
    core,
  };
}

/** Parses and validates a mix. Throws `MixError` listing every problem found. */
export function parseMix(input: string | unknown): Mix {
  const c = new Check();
  let value: unknown = input;
  if (typeof input === "string") {
    try {
      value = JSON.parse(input);
    } catch (error) {
      throw new MixError([`not JSON: ${error instanceof Error ? error.message : String(error)}`]);
    }
  }
  const top = c.object(value, "mix", ["version", "tenant", "journeys", "requests"]);
  if (top === undefined) throw new MixError(c.problems);
  if (top["version"] !== 1) c.bad("mix.version", "must be 1");
  let host: string | undefined;
  let realm: string | undefined;
  if (top["tenant"] !== undefined) {
    const t = c.object(top["tenant"], "mix.tenant", ["host", "realm"]);
    if (t?.["host"] !== undefined) {
      host = c.string(t["host"], "mix.tenant.host");
      if (host !== undefined && !/^[a-z0-9.-]+(?::\d+)?$/i.test(host)) c.bad("mix.tenant.host", "must be a bare host name");
    }
    if (t?.["realm"] !== undefined) realm = c.string(t["realm"], "mix.tenant.realm");
  }
  const journeys: Record<string, JourneySpec> = {};
  if (top["journeys"] !== undefined) {
    const j = c.object(top["journeys"], "mix.journeys", Object.keys((top["journeys"] ?? {}) as object));
    for (const [tree, spec] of Object.entries(j ?? {})) {
      const parsed = journey(c, tree, spec, `mix.journeys[${JSON.stringify(tree)}]`);
      if (parsed !== undefined) journeys[tree] = parsed;
    }
  }
  const requests: RequestSpec[] = [];
  if (top["requests"] !== undefined) {
    const arr = c.array(top["requests"], "mix.requests") ?? [];
    for (const [i, raw] of arr.entries()) {
      const parsed = request(c, raw, `mix.requests[${i}]`);
      if (parsed !== undefined) requests.push(parsed);
    }
    const names = requests.map((r) => r.name);
    for (const n of new Set(names.filter((x, i) => names.indexOf(x) !== i))) c.bad("mix.requests", `name ${JSON.stringify(n)} is used twice`);
  }
  if (Object.keys(journeys).length === 0 && requests.length === 0 && c.problems.length === 0) {
    c.bad("mix", "has no journeys and no requests, so there would be no traffic");
  }
  if (c.problems.length > 0) throw new MixError(c.problems);
  return { tenant: { host, realm }, journeys, requests };
}

const WEEK_HOURS = 168;

/** Hour of the week (Monday 00:00 UTC is 0) and hour of the day, for a time in nanoseconds. */
export function hourOfWeek(ns: bigint): { week: number; day: number; dow: number } {
  const hours = Number(ns / 3_600_000_000_000n);
  const days = Math.floor(hours / 24);
  const dow = (((days + 3) % 7) + 7) % 7; // 1970-01-01 was a Thursday
  const day = ((hours % 24) + 24) % 24;
  return { week: dow * 24 + day, day, dow };
}

/** The curve's rate, per virtual hour, at time `ns`. */
export function ratePerHour(curve: RateCurve, ns: bigint): number {
  const { week, day, dow } = hourOfWeek(ns);
  if (curve.kind === "hours") return curve.byHourOfWeek[week % WEEK_HOURS] ?? 0;
  return curve.perHour * (curve.daily[day] ?? 1) * (curve.weekly[dow] ?? 1);
}

/** The synthetic person with index `i` (0-based): `user0001`, `user0001@example.com`, a fixed id. */
export function person(spec: Users, i: number): { username: string; email: string; userId: string } {
  const n = String(i + 1).padStart(Math.max(4, String(spec.count).length), "0");
  return {
    username: `user${n}`,
    email: `user${n}@${spec.domain}`,
    userId: `00000000-0000-4000-8000-${(i + 1).toString(16).padStart(12, "0")}`,
  };
}
