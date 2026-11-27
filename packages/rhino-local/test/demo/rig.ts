/** Shared fixtures for the demo driver tests: a tiny generic tree and a recording sink. */
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { JourneyConfig, JourneyEmulator, type JourneyOptions } from "../../src/journey/index.ts";
import { SUCCESS_NODE_ID, FAILURE_NODE_ID } from "../../src/aic/constants.ts";
import { Driver, manualPacer, type Report } from "../../src/demo/driver.ts";
import { parseMix, type Mix } from "../../src/demo/mix.ts";
import type { Break } from "../../src/demo/breaks.ts";
import type { LogRecord, LogSink } from "../../src/logs/record.ts";
import { parseRfc3339Ns } from "../../src/logs/time.ts";
import type { RhinoRunner } from "../../src/runner.ts";

export const T0 = parseRfc3339Ns("2026-11-18T00:00:00Z");
export const HOUR = 3_600_000_000_000n;
export const MINUTE = 60_000_000_000n;

const script = (source: string) => ({
  name: "demo-script",
  evaluatorVersion: "2.0",
  language: "JAVASCRIPT",
  script: Buffer.from(source).toString("base64"),
});

/** `login` asks for a name, then routes on it; `check` is a second decision after it. */
export function config(): JourneyConfig {
  return new JourneyConfig({
    realm: "alpha",
    trees: {
      "tree-a": {
        entryNodeId: "login",
        nodes: {
          login: { nodeType: "ScriptedDecisionNode", displayName: "Login", connections: { true: "check", false: FAILURE_NODE_ID } },
          check: { nodeType: "ScriptedDecisionNode", displayName: "Check", connections: { true: SUCCESS_NODE_ID, false: FAILURE_NODE_ID } },
        },
      },
    },
    nodes: {
      ScriptedDecisionNode: {
        login: { script: "ask", outcomes: ["true", "false"] },
        check: { script: "pass", outcomes: ["true", "false"] },
      },
    },
    scripts: {
      ask: script(
        "if(callbacks.isEmpty()){callbacksBuilder.nameCallback('Name');}else{var n=callbacks.getNameCallbacks().get(0);if(String(n).indexOf('user')===0){nodeState.putShared('username',String(n));action.goTo('true');}else{action.goTo('false');}}",
      ),
      pass: script("action.goTo('true');"),
    },
  });
}

export function makeEngineFor(runner: RhinoRunner, cfg: JourneyConfig): (options: JourneyOptions) => JourneyEmulator {
  const idm = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }), runner);
  return (options) => new JourneyEmulator(cfg, runner, idm, options);
}

export class Recording implements LogSink {
  readonly records: LogRecord[] = [];
  emit(record: LogRecord): void {
    this.records.push(record);
  }
}

export function mixJson(overrides: { perHour?: number; abandon?: number; requests?: unknown[] } = {}): unknown {
  return {
    version: 1,
    tenant: { host: "tenant.example.com" },
    journeys: {
      "tree-a": {
        rate: { perHour: overrides.perHour ?? 600 },
        users: { count: 50, domain: "example.com" },
        networks: [{ cidr: "198.51.100.0/24", share: 1 }],
        latencyMs: { medianMs: 30, p95Ms: 90 },
        abandon: overrides.abandon ?? 0.1,
        steps: [
          {
            thinkSeconds: [1, 5],
            answers: [
              { label: "good", outcome: "success", share: 0.8, fill: { NameCallback: "{username}" } },
              { label: "bad", outcome: "failure", share: 0.2, fill: { NameCallback: "intruder" } },
            ],
          },
        ],
      },
    },
    requests: overrides.requests ?? [
      {
        name: "token",
        stream: "am-access",
        method: "POST",
        path: "/am/oauth2/realms/root/realms/alpha/access_token",
        rate: { perHour: 300 },
        networks: [{ cidr: "198.51.100.0/24", share: 1 }],
        status: [
          { share: 0.95, status: "SUCCESSFUL", statusCode: 200 },
          { share: 0.05, status: "FAILED", statusCode: 401 },
        ],
      },
      {
        name: "read",
        stream: "idm-access",
        method: "GET",
        path: "/openidm/managed/alpha_user/{userId}",
        rate: { perHour: 200 },
        networks: [{ cidr: "198.51.100.0/24", share: 1 }],
        status: [{ share: 1, status: "SUCCESSFUL", statusCode: 200 }],
      },
    ],
  };
}

export type RunOptions = {
  mix?: Mix;
  breaks?: readonly Break[];
  seed?: string;
  duration?: bigint;
  start?: bigint;
  concurrency?: number;
  scale?: number;
  config?: JourneyConfig;
};

export async function runDriver(runner: RhinoRunner, o: RunOptions = {}): Promise<{ records: LogRecord[]; report: Report; start: bigint; end: bigint }> {
  const cfg = o.config ?? config();
  const sink = new Recording();
  const start = o.start ?? T0;
  const end = start + (o.duration ?? HOUR);
  const driver = new Driver({
    mix: o.mix ?? parseMix(mixJson()),
    config: cfg,
    makeEngine: makeEngineFor(runner, cfg),
    sink,
    pacer: manualPacer(start),
    seed: o.seed ?? "test",
    breaks: o.breaks ?? [],
    concurrency: o.concurrency ?? 8,
    ...(o.scale === undefined ? {} : { scale: o.scale }),
  });
  const report = await driver.run({ until: end });
  return { records: sink.records, report, start, end };
}

export const ts = (r: LogRecord): bigint => parseRfc3339Ns(r.timestamp);
// Payloads are arbitrary JSON; tests index into them by field.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const payload = (r: LogRecord): Record<string, any> => r.payload as Record<string, any>;
/** A canonical form for comparing runs: arrival order of equal timestamps may differ. */
export const canon = (records: readonly LogRecord[]): string[] => records.map((r) => JSON.stringify(r)).sort();
