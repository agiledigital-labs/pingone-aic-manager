import { afterAll, beforeAll, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import {
  JourneyConfig,
  JourneyEmulator,
  isoMillis,
  isoNanos,
  type JourneyOptions,
  type TreeNode,
} from "../../src/journey/index.ts";
import type { LogRecord } from "../../src/logs/record.ts";
import { SUCCESS_NODE_ID as SUCCESS, FAILURE_NODE_ID as FAILURE } from "../../src/aic/constants.ts";
import { fillCallbackInputs } from "../../src/aic/callbacks.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});

type Step = { source: string; routes?: Record<string, string>; outcomes?: string[] };
/** A tree of generic scripted decision nodes: `routes` maps an outcome to the next node id. */
function make(steps: Record<string, Step>, entry: string, options: JourneyOptions = {}) {
  const nodes: Record<string, TreeNode> = {};
  const configs: Record<string, { script: string; outcomes: string[] }> = {};
  const scripts: Record<string, unknown> = {};
  for (const [id, step] of Object.entries(steps)) {
    nodes[id] = {
      nodeType: "ScriptedDecisionNode",
      displayName: `Step ${id}`,
      connections: step.routes ?? { true: SUCCESS, false: FAILURE },
    };
    configs[id] = { script: `script-${id}`, outcomes: step.outcomes ?? ["true", "false"] };
    scripts[`script-${id}`] = {
      name: `name-${id}`,
      evaluatorVersion: "2.0",
      language: "JAVASCRIPT",
      script: Buffer.from(step.source).toString("base64"),
    };
  }
  return new JourneyEmulator(
    new JourneyConfig({
      realm: "alpha",
      trees: { invented: { entryNodeId: entry, nodes } },
      nodes: { ScriptedDecisionNode: configs },
      scripts: scripts as never,
    }),
    runner,
    new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }), runner),
    options,
  );
}
function sinkAndClock(step = 1_000_000n) {
  const records: LogRecord[] = [];
  let ticks = 0n;
  return {
    records,
    options: {
      logSink: { emit: (record: LogRecord) => records.push(record) },
      // Every reading moves the clock on, so each record gets its own, ordered, timestamp.
      now: () => 1_795_000_000_000_000_000n + ticks++ * step,
    } satisfies JourneyOptions,
  };
}
const post = (emu: JourneyEmulator, body = {}, extra: Record<string, unknown> = {}) =>
  emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "invented" },
    body,
    clientIp: "192.0.2.7",
    ...extra,
  });
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const payload = (record: LogRecord) => record.payload as Record<string, any>;
const events = (records: LogRecord[]) =>
  records.map((r) => `${r.source}:${payload(r).eventName ?? payload(r).level}`);
const nodeRecords = (records: LogRecord[]) =>
  records.filter((r) => payload(r).eventName === "AM-NODE-LOGIN-COMPLETED");
const treeRecords = (records: LogRecord[]) =>
  records.filter((r) => payload(r).eventName === "AM-TREE-LOGIN-COMPLETED");
const info = (record: LogRecord) => payload(record).entries[0].info;

const ASK = "callbacksBuilder.nameCallback('Name');";
const RESUME =
  "if(callbacks.isEmpty()){" +
  ASK +
  "}else{nodeState.putShared('username',callbacks.getNameCallbacks().get(0));action.goTo('true');}";
const THREE = {
  a: { source: "logger.info('first');action.goTo('true');", routes: { true: "b" } },
  b: { source: RESUME, routes: { true: "c" } },
  c: { source: "action.goTo('true');", routes: { true: SUCCESS } },
};
const answer = (body: unknown) =>
  JSON.parse(fillCallbackInputs(body as never, [{ type: "NameCallback", value: "pat" }], "pat"));

it("a three node journey writes its records in the tenant's order across two round trips", async () => {
  const { records, options } = sinkAndClock();
  const emu = make(THREE, "a", options);
  const first = await post(emu, {}, { transactionId: "caller-tx" });
  expect(first.status).toBe(200);
  expect(events(records)).toEqual([
    "am-access:AM-ACCESS-ATTEMPT",
    "am-core:INFO",
    "am-authentication:AM-NODE-LOGIN-COMPLETED",
    "am-access:AM-ACCESS-OUTCOME",
  ]);
  const firstCount = records.length;
  const second = await post(emu, answer(first.body), { transactionId: "caller-tx-2" });
  expect(second.status).toBe(200);
  expect(events(records.slice(firstCount))).toEqual([
    "am-access:AM-ACCESS-ATTEMPT",
    "am-authentication:AM-NODE-LOGIN-COMPLETED",
    "am-authentication:AM-NODE-LOGIN-COMPLETED",
    "am-core:INFO", // session hook lines, measured on a failed tree and assumed for a success
    "am-core:INFO",
    "am-activity:AM-SESSION-CREATED",
    "am-authentication:AM-TREE-LOGIN-COMPLETED",
    "am-access:AM-ACCESS-OUTCOME",
  ]);
  // Node records, in execution order, with the measured keys and nothing else.
  const nodes = nodeRecords(records);
  expect(nodes.map((r) => info(r).nodeId)).toEqual(["a", "b", "c"]);
  expect(nodes.map((r) => Object.keys(info(r)))).toEqual([
    ["authLevel", "displayName", "nodeId", "nodeOutcome", "nodeType", "treeName"],
    ["authLevel", "displayName", "nodeId", "nodeOutcome", "nodeType", "treeName"],
    ["authLevel", "displayName", "nodeId", "nodeOutcome", "nodeType", "treeName"],
  ]);
  expect(info(nodes[0]!)).toEqual({
    authLevel: "0",
    displayName: "Step a",
    nodeId: "a",
    nodeOutcome: "true",
    nodeType: "ScriptedDecisionNode",
    treeName: "invented",
  });
  // The node that sent the callbacks wrote nothing until it completed; it is not duplicated.
  expect(nodes.filter((r) => info(r).nodeId === "b")).toHaveLength(1);
  // Envelope and payload fields of a node record.
  const record = nodes[0]!;
  expect(record.source).toBe("am-authentication");
  expect(record.type).toBe("application/json");
  expect(payload(record)).toMatchObject({
    component: "Authentication",
    eventName: "AM-NODE-LOGIN-COMPLETED",
    level: "INFO",
    realm: "/alpha",
    source: "audit",
    topic: "authentication",
    transactionId: "caller-tx/0",
  });
  expect(payload(record)._id).toMatch(/^[0-9a-f-]{36}-\d+$/);
  expect(payload(record).timestamp).toBe(isoMillis(BigInt(Date.parse(payload(record).timestamp)) * 1_000_000n));
  expect(record.timestamp.startsWith(payload(record).timestamp.slice(0, 19))).toBe(true);
  // The tree record closes the journey, successful, ahead of the access outcome.
  const tree = treeRecords(records);
  expect(tree).toHaveLength(1);
  expect(payload(tree[0]!).result).toBe("SUCCESSFUL");
  expect(info(tree[0]!).treeName).toBe("invented");
  // Everything from one journey carries one trackingIds[0]: both round trips, every stream that has one.
  const tracking = records
    .filter((r) => payload(r).trackingIds)
    .map((r) => payload(r).trackingIds[0] as string);
  expect(tracking.length).toBeGreaterThanOrEqual(6);
  expect(new Set(tracking).size).toBe(1);
  expect(tracking[0]).toMatch(/^[0-9a-f-]{36}-\d+$/);
  // The caller's transaction id is `<id>/0`; the second request supplied its own.
  expect(records.slice(0, firstCount).every((r) => payload(r).transactionId === "caller-tx/0")).toBe(true);
  expect(records.slice(firstCount).every((r) => payload(r).transactionId === "caller-tx-2/0")).toBe(true);
  // Records are in nondecreasing time, and the envelope carries nanoseconds.
  const stamps = records.map((r) => r.timestamp);
  // Compared as numbers: trimmed zeros make ".01Z" sort before ".009Z" as text.
  const nanos = (stamp: string) =>
    BigInt(Date.parse(stamp.replace(/\.\d+Z$/, "Z"))) * 1_000_000n +
    BigInt(((/\.(\d+)Z$/.exec(stamp)?.[1] ?? "") + "000000000").slice(0, 9));
  const parsed = stamps.map(nanos);
  expect([...parsed].sort((x, y) => (x < y ? -1 : x > y ? 1 : 0))).toEqual(parsed);
  expect(stamps.every((s) => /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d{1,9})?Z$/.test(s))).toBe(true);
});

it("nodeOutcome is a string, whatever the outcome (a boolean is dropped by the consumer)", async () => {
  const { records, options } = sinkAndClock();
  await post(
    make({ a: { source: "action.goTo('true');", routes: { true: "b" } }, b: { source: "action.goTo('false');" } }, "a", options),
  );
  const outcomes = nodeRecords(records).map((r) => info(r).nodeOutcome);
  expect(outcomes).toEqual(["true", "false"]);
  for (const outcome of outcomes) expect(typeof outcome).toBe("string");
  // A custom outcome name is the string too, as measured.
  const custom = sinkAndClock();
  await post(
    make({ a: { source: "action.goTo('KDEMO-OUT');", outcomes: ["KDEMO-OUT"], routes: { "KDEMO-OUT": SUCCESS } } }, "a", custom.options),
  );
  expect(info(nodeRecords(custom.records)[0]!).nodeOutcome).toBe("KDEMO-OUT");
});

it("a second journey gets its own trackingId, and a resumed one keeps its", async () => {
  const { records, options } = sinkAndClock();
  const emu = make(THREE, "a", options);
  const one = await post(emu);
  const two = await post(emu);
  const resumed = await post(emu, answer(one.body));
  expect(resumed.status).toBe(200);
  const tracked = (from: number, to: number) =>
    new Set(
      records
        .slice(from, to)
        .filter((r) => payload(r).trackingIds)
        .map((r) => payload(r).trackingIds[0]),
    );
  const outcomes = records
    .map((r, index) => [r, index] as const)
    .filter(([r]) => payload(r).eventName === "AM-ACCESS-OUTCOME")
    .map(([, index]) => index);
  const journeyOne = payload(records[outcomes[0]!]!).trackingIds[0];
  const journeyTwo = payload(records[outcomes[1]!]!).trackingIds[0];
  expect(journeyOne).not.toBe(journeyTwo);
  // The resume of journey one carries journey one's id on every record that has one.
  expect([...tracked(outcomes[1]! + 1, records.length)]).toEqual([journeyOne]);
  expect(two.status).toBe(200);
});

it("a node routed to Failure writes its record, then a FAILED tree record with the reason", async () => {
  const { records, options } = sinkAndClock();
  const result = await post(make({ a: { source: "logger.error('nope');action.goTo('false');" } }, "a", options));
  expect(result.status).toBe(401);
  expect(events(records)).toEqual([
    "am-access:AM-ACCESS-ATTEMPT",
    "am-core:ERROR",
    "am-authentication:AM-NODE-LOGIN-COMPLETED",
    "am-core:INFO",
    "am-core:INFO",
    "am-authentication:AM-TREE-LOGIN-COMPLETED",
    "am-core:WARN",
    "am-access:AM-ACCESS-OUTCOME",
  ]);
  expect(info(nodeRecords(records)[0]!).nodeOutcome).toBe("false");
  const tree = treeRecords(records)[0]!;
  expect(payload(tree).result).toBe("FAILED");
  expect(info(tree)).toEqual({
    authLevel: "0",
    ipAddress: "192.0.2.7",
    nodeExtraLogging: { failureReason: "Login failure" },
    treeName: "invented",
  });
  const warn = records.at(-2)!;
  expect(payload(warn).message).toBe("Authentication encountered an error: [Status: 401 Unauthorized]");
});

it("a throwing script writes no node record; its logger lines before the throw stay", async () => {
  const { records, options } = sinkAndClock();
  const result = await post(
    make({ a: { source: "logger.warn('KEPT');throw new Error('boom');" } }, "a", options),
  );
  expect(result.status).toBe(401);
  expect(nodeRecords(records)).toHaveLength(0);
  const core = records.filter((r) => r.source === "am-core");
  const framework = core.filter((r) => !String(payload(r).logger).startsWith("scripts."));
  expect(
    framework.filter((r) => ["ERROR", "WARN"].includes(payload(r).level)).map((r) => [payload(r).level, payload(r).message]),
  ).toEqual([
    ["ERROR", "Script 'name-a' with evaluatorVersion 2.0 in realm /alpha terminated with exception"],
    ["WARN", "error evaluating the script"],
    ["WARN", "An error occurred during scripted node processing"],
    ["WARN", "Authentication tree failed with exception"],
    ["WARN", "Authentication encountered an error: [Status: 401 Unauthorized]"],
  ]);
  const kept = core.find((r) => payload(r).message === "KEPT")!;
  expect(payload(kept).level).toBe("WARN");
  // The kept line precedes the ERROR in the stream, as it ran first.
  expect(records.indexOf(kept)).toBeLessThan(records.findIndex((r) => payload(r).level === "ERROR"));
  const tree = treeRecords(records)[0]!;
  expect(payload(tree).result).toBe("FAILED");
  expect(info(tree).nodeExtraLogging).toEqual({
    exception: "An exception occurred during the authentication process",
    failureReason:
      "javax.script.ScriptException: java.util.concurrent.ExecutionException: javax.script.ScriptException: Error: boom (name-a#1) in name-a at line number 1 at column number 0",
  });
  // A thrown string reads without the Error prefix.
  const text = sinkAndClock();
  await post(make({ a: { source: "throw 'plain';" } }, "a", text.options));
  expect(info(treeRecords(text.records)[0]!).nodeExtraLogging.failureReason).toContain("ScriptException: plain (name-a#1)");
});

it("script logger lines carry the measured payload and levels; trace is not written", async () => {
  const { records, options } = sinkAndClock();
  await post(
    make(
      {
        a: {
          source:
            "logger.trace('t');logger.debug('d');logger.info('hello {}', 'there');logger.warn('w');logger.error('e');action.goTo('true');",
        },
      },
      "a",
      options,
    ),
    {},
    { transactionId: "tx-logger" },
  );
  const lines = records.filter((r) => String(payload(r).logger ?? "").startsWith("scripts."));
  expect(lines.map((r) => [payload(r).level, payload(r).message])).toEqual([
    ["DEBUG", "d"],
    ["INFO", "hello there"],
    ["WARN", "w"],
    ["ERROR", "e"],
  ]);
  const first = lines[0]!;
  expect(first.source).toBe("am-core");
  expect(payload(first)).toMatchObject({
    context: "default",
    logger: "scripts.AUTHENTICATION_TREE_DECISION_NODE.script-a.(name-a)",
    mdc: { transactionId: "tx-logger/0" },
    transactionId: "tx-logger/0",
  });
  expect(payload(first).thread).toMatch(/^ScriptEvaluator-\d+$/);
  expect(Object.keys(payload(first))).toEqual(
    ["context", "level", "logger", "mdc", "message", "thread", "timestamp", "transactionId"],
  );
});

it("access attempt and outcome pair up, with status, status code and elapsed time", async () => {
  // A script's HTTP call moves the injected clock 250 ms: the elapsed time is the emulator's own measurement.
  const { records, options } = sinkAndClock(1n);
  let clock = 1_795_000_000_000_000_000n;
  const emu = make(
    { a: { source: "httpClient.send('https://svc.example.com/x').get();action.goTo('false');" } },
    "a",
    {
      logSink: options.logSink,
      now: () => clock,
      http: () => {
        clock += 250_000_000n;
        return { status: 200, body: {} };
      },
    },
  );
  await post(emu, {}, { url: "https://tenant.example.com/am/json/realms/root/realms/alpha/authenticate", userAgent: "agent/1", headers: { Host: ["tenant.example.com"], Cookie: ["secret=1"] } });
  const [attempt, outcome] = [records[0]!, records.at(-1)!];
  expect(payload(attempt).eventName).toBe("AM-ACCESS-ATTEMPT");
  expect(payload(attempt).trackingIds).toBeUndefined();
  expect(payload(attempt).client).toEqual({ ip: "192.0.2.7" });
  expect(payload(attempt).http.request).toMatchObject({
    method: "POST",
    path: "https://tenant.example.com/am/json/realms/root/realms/alpha/authenticate",
    queryParameters: { authIndexType: ["service"], authIndexValue: ["invented"] },
    secure: true,
  });
  expect(payload(attempt).http.request.headers["user-agent"]).toEqual(["agent/1"]);
  expect(payload(attempt).http.request.headers.cookie).toBeUndefined();
  expect(payload(outcome).eventName).toBe("AM-ACCESS-OUTCOME");
  expect(payload(outcome).response).toEqual({
    detail: { reason: "Unauthorized" },
    elapsedTime: 250,
    elapsedTimeUnits: "MILLISECONDS",
    status: "FAILED",
    statusCode: "401",
  });
  expect(payload(outcome).trackingIds).toEqual(payload(records.find((r) => payload(r).eventName === "AM-TREE-LOGIN-COMPLETED")!).trackingIds);
  expect(payload(outcome).client).toEqual({ ip: "192.0.2.7" });
  expect(payload(outcome).transactionId).toBe(payload(attempt).transactionId);
  // An intermediate (callback) response is SUCCESSFUL / "200" with no detail, as measured.
  const step = sinkAndClock();
  await post(make(THREE, "a", { ...step.options, latencyMs: () => 42 }));
  expect(payload(step.records.at(-1)!).response).toEqual({
    elapsedTime: 42,
    elapsedTimeUnits: "MILLISECONDS",
    status: "SUCCESSFUL",
    statusCode: "200",
  });
  // The path falls back to the configured origin when the request has none.
  const fallback = sinkAndClock();
  await post(make(THREE, "a", { ...fallback.options, origin: "https://other.example.com" }));
  expect(payload(fallback.records[0]!).http.request.path).toBe(
    "https://other.example.com/am/json/realms/root/realms/alpha/authenticate",
  );
});

it("an id in the x-forgerock-transactionid header becomes <id>/0; the field wins over it", async () => {
  const header = sinkAndClock();
  await post(make(THREE, "a", header.options), {}, { headers: { "X-ForgeRock-TransactionId": ["from-header"] } });
  expect(new Set(header.records.map((r) => payload(r).transactionId))).toEqual(new Set(["from-header/0"]));
  const both = sinkAndClock();
  await post(make(THREE, "a", both.options), {}, { transactionId: "field", headers: { "x-forgerock-transactionid": ["from-header"] } });
  expect(new Set(both.records.map((r) => payload(r).transactionId))).toEqual(new Set(["field/0"]));
  // Without one, every record of the request still shares a single id.
  const none = sinkAndClock();
  await post(make(THREE, "a", none.options));
  expect(new Set(none.records.map((r) => payload(r).transactionId)).size).toBe(1);
});

it("an abandoned journey writes nothing after its last response", async () => {
  const { records, options } = sinkAndClock();
  const emu = make(THREE, "a", options);
  await post(emu);
  const count = records.length;
  expect(treeRecords(records)).toHaveLength(0);
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(records).toHaveLength(count);
});

it("with no sink the responses are the same and nothing is emitted", async () => {
  const plain = make(THREE, "a");
  const { records, options } = sinkAndClock();
  const logged = make(THREE, "a", options);
  const [a, b] = [await post(plain), await post(logged)];
  expect({ status: a.status, keys: Object.keys(a.body as object) }).toEqual({
    status: b.status,
    keys: Object.keys(b.body as object),
  });
  expect(records.length).toBeGreaterThan(0);
  const failing = await post(make({ a: { source: "throw new Error('x');" } }, "a"));
  expect(failing).toMatchObject({ status: 401, body: { message: "Login failure" } });
});

// auditEntryDetail: an output the script assigns (measured 2026-11-25).
const audit = async (source: string, outcome = "true") => {
  const { records, options } = sinkAndClock();
  const result = await post(
    make({ a: { source: `${source}\naction.goTo('${outcome}');` } }, "a", options),
  );
  return { records, result };
};
it.each([
  ["a string", "auditEntryDetail = 'KDEMO-STR';", "KDEMO-STR"],
  ["an object, keys sorted and undefined as null", "auditEntryDetail = {z: 1, a: undefined, m: {y: 'y', b: ['a', 'b']}, e: '', n: null, t: true, f: 1.5};", { a: null, e: "", f: 1.5, m: { b: ["a", "b"], y: "y" }, n: null, t: true, z: 1 }],
  ["an empty object", "auditEntryDetail = {};", {}],
  ["a top-level var", "var auditEntryDetail = {k: 'v'};", { k: "v" }],
])("auditEntryDetail %s goes to the node record as auditInfo", async (_name, source, expected) => {
  const { records } = await audit(source);
  expect(info(nodeRecords(records)[0]!).nodeExtraLogging).toEqual({ auditInfo: expected });
  // Keys come out sorted at every depth.
  const sortedKeys = (value: unknown): boolean =>
    value === null || typeof value !== "object"
      ? true
      : Array.isArray(value)
        ? value.every(sortedKeys)
        : Object.keys(value).join() === Object.keys(value).sort().join() && Object.values(value).every(sortedKeys);
  expect(sortedKeys(info(nodeRecords(records)[0]!).nodeExtraLogging)).toBe(true);
});
it("auditEntryDetail null, undefined or never assigned leaves no nodeExtraLogging", async () => {
  for (const source of ["auditEntryDetail = null;", "auditEntryDetail = undefined;", "var auditEntryDetail;", ""]) {
    const { records } = await audit(source);
    expect(Object.keys(info(nodeRecords(records)[0]!))).not.toContain("nodeExtraLogging");
  }
});
it("auditEntryDetail rides a declared false outcome too", async () => {
  const { records } = await audit("auditEntryDetail = {k: 'v'};", "false");
  expect(info(nodeRecords(records)[0]!)).toMatchObject({ nodeOutcome: "false", nodeExtraLogging: { auditInfo: { k: "v" } } });
  // The Failure node's tree record carries no auditInfo (measured).
  expect(info(treeRecords(records)[0]!).nodeExtraLogging).toEqual({ failureReason: "Login failure" });
});
it.each([
  ["an array", "auditEntryDetail = ['a', 'b'];"],
  ["a number", "auditEntryDetail = 7;"],
  ["a boolean", "auditEntryDetail = true;"],
])("auditEntryDetail %s fails the tree: 401, a WARN, the reason, no node record", async (_name, source) => {
  const { records, result } = await audit(source);
  expect(result.status).toBe(401);
  expect(nodeRecords(records)).toHaveLength(0);
  expect(
    records.some(
      (r) => payload(r).level === "WARN" && payload(r).message === "script auditEntryDetail not type String or Map",
    ),
  ).toBe(true);
  expect(info(treeRecords(records)[0]!).nodeExtraLogging).toEqual({
    exception: "An exception occurred during the authentication process",
    failureReason: "Invalid auditEntryDetail type from script",
  });
  expect(payload(treeRecords(records)[0]!).result).toBe("FAILED");
});
it("auditEntryDetail assigned and then a throw is not emitted anywhere", async () => {
  const { records } = await audit("auditEntryDetail = {k: 'LEAK'}; throw new Error('after');");
  expect(nodeRecords(records)).toHaveLength(0);
  expect(JSON.stringify(records.filter((r) => r.source === "am-authentication"))).not.toContain("LEAK");
});
it("only an assignment to the script variable counts; nodeState writes do not", async () => {
  const { records } = await audit(
    "nodeState.putShared('auditEntryDetail', {k: 'SHARED'}); nodeState.putTransient('nodeExtraLogging', 'T');",
  );
  expect(Object.keys(info(nodeRecords(records)[0]!))).not.toContain("nodeExtraLogging");
  const assigned = await audit("auditEntryDetail = {k: 'ASSIGNED'};");
  expect(info(nodeRecords(assigned.records)[0]!).nodeExtraLogging.auditInfo).toEqual({ k: "ASSIGNED" });
});
it("a value assigned in one node does not leak into the next node's record", async () => {
  const { records, options } = sinkAndClock();
  await post(
    make(
      {
        a: { source: "auditEntryDetail = 'ONE';action.goTo('true');", routes: { true: "b" } },
        b: { source: "action.goTo('true');", routes: { true: SUCCESS } },
      },
      "a",
      options,
    ),
  );
  const [one, two] = nodeRecords(records);
  expect(info(one!).nodeExtraLogging).toEqual({ auditInfo: "ONE" });
  expect(Object.keys(info(two!))).not.toContain("nodeExtraLogging");
});

it("timestamps: the payload is milliseconds, the envelope nanoseconds with trailing zeros trimmed", () => {
  const ns = 1_795_000_000_123_456_700n;
  expect(isoMillis(ns)).toBe("2026-11-18T11:06:40.123Z");
  expect(isoNanos(ns)).toBe("2026-11-18T11:06:40.1234567Z");
  expect(isoNanos(1_795_000_000_000_000_000n)).toBe("2026-11-18T11:06:40Z");
});

// Measured 2026-11-25: an identified user's success record names the user's `_id` in `userId`,
// while `principal` carries the username. The discriminating case: a userId built from the
// username (right only for the anonymous user) fails here.
it("a successful tree record carries the measured principal and userId", async () => {
  const { records, options } = sinkAndClock();
  const emu = make(
    {
      a: {
        source:
          "nodeState.putShared('username', 'user-a@example.com'); nodeState.putShared('_id', '00000000-0000-4000-8000-0000000000aa'); action.goTo('true');",
      },
    },
    "a",
    options,
  );
  expect((await post(emu)).status).toBe(200);
  const tree = treeRecords(records);
  expect(tree).toHaveLength(1);
  const p = payload(tree[0]!);
  expect(p.result).toBe("SUCCESSFUL");
  expect(p.principal).toEqual(["user-a@example.com"]);
  expect(p.userId).toBe("id=00000000-0000-4000-8000-0000000000aa,ou=user,o=alpha,ou=services,ou=am-config");
  expect(Object.keys(info(tree[0]!)).sort()).toEqual(["authLevel", "ipAddress", "treeName"]);
  // The session record sits between the node record and the tree record, and names the journey.
  const events = records.filter((r) => r.source !== "am-core").map((r) => payload(r).eventName);
  expect(events).toEqual([
    "AM-ACCESS-ATTEMPT",
    "AM-NODE-LOGIN-COMPLETED",
    "AM-SESSION-CREATED",
    "AM-TREE-LOGIN-COMPLETED",
    "AM-ACCESS-OUTCOME",
  ]);
  const session = records.find((r) => payload(r).eventName === "AM-SESSION-CREATED")!;
  expect(session.source).toBe("am-activity");
  expect(payload(session)).toMatchObject({
    component: "Session",
    topic: "activity",
    operation: "CREATE",
    objectId: p.trackingIds[0],
    runAs: p.userId,
    userId: p.userId,
  });
});
