import { expectEmulationFailure } from "../src/harness/emulation-guard.ts";
/** Resource and shutdown checks on the actual JVM stdin/stdout protocol. */
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { beforeAll, expect, it } from "vitest";
import { cacheDir, ensureRhinoJar, ensureRunnerClasses, hostLaunch } from "../src/jvm.ts";
import { RhinoRunner } from "../src/runner.ts";
import type { HostCall, JobResponse, JsonValue } from "../src/protocol.ts";

let classes: string;
let jar: string;
beforeAll(async () => {
  jar = await ensureRhinoJar(cacheDir());
  classes = await ensureRunnerClasses(jar, { cache: cacheDir() });
}, 60_000);

type Record = HostCall | JobResponse;
async function raw(args: string[], check: (client: {
  send: (job: object) => void;
  wait: (id: string, call?: boolean) => Promise<Record>;
  records: Record[];
}) => Promise<void>): Promise<void> {
  const launch = hostLaunch(classes, jar);
  const child = spawn(launch.command, [...launch.args, ...args], { env: launch.env, stdio: "pipe" });
  const records: Record[] = [];
  const watchers = new Set<() => void>();
  const lines = createInterface({ input: child.stdout });
  lines.on("line", (line) => {
    records.push(JSON.parse(line) as Record);
    for (const watcher of watchers) watcher();
  });
  child.stderr.resume();
  const exited = new Promise<void>((resolve) => child.on("exit", () => resolve()));
  try {
    await check({
      records,
      send: (job) => { child.stdin.write(`${JSON.stringify(job)}\n`); },
      wait: (id, call = false) => new Promise<Record>((resolve, reject) => {
        const timer = setTimeout(() => { watchers.delete(poll); reject(new Error(`no record for ${id}`)); }, 4000);
        const poll = () => {
          const record = records.find((record) => record.id === id && (("type" in record) === call));
          if (record) { clearTimeout(timer); watchers.delete(poll); resolve(record); }
        };
        watchers.add(poll);
        poll();
      }),
    });
  } finally {
    child.stdin.end();
    // Also terminates an unfixed runner stuck in a no-timeout host call.
    child.kill("SIGKILL");
    await exited;
    lines.close();
  }
}

it("#11 refuses excess concurrent jobs immediately and keeps accepted jobs alive", async () => {
  await raw(["--max-concurrent-jobs=2"], async ({ send, wait }) => {
    for (const id of ["blocked-a", "blocked-b"]) {
      send({ id, engine: "idm", source: "openidm.read('blocked')", timeoutMs: 0 });
      await wait(id, true);
    }
    send({ id: "excess", source: "42" });
    const excess = await wait("excess");
    expect(excess).toMatchObject({ outcome: "protocol_error", error: { message: "rhino-local: concurrent job limit exceeded (2)" } });
    for (const id of ["blocked-a", "blocked-b"]) send({ type: "reply", id, seq: 1, result: 42 });
    expect(await wait("blocked-a")).toMatchObject({ outcome: "ok", value: 42 });
    expect(await wait("blocked-b")).toMatchObject({ outcome: "ok", value: 42 });
  });
});

it("#11 re-entry at the admission limit refuses instead of queuing or deadlocking", async () => {
    expectEmulationFailure(/concurrent job limit exceeded/);
  const runner = await RhinoRunner.spawn({ maxConcurrentJobs: 2 });
  const outcomes: string[] = [];
  const evaluate = async (): Promise<JsonValue> => {
    const result = await runner.eval({ engine: "idm", source: "openidm.read('nest')", timeoutMs: 10_000 }, {
      onCall: () => evaluate(),
    });
    outcomes.push(result.outcome);
    return result.outcome === "protocol_error" ? 42 : result.value;
  };
  try {
    expect(await evaluate()).toBeNull();
    expect(outcomes).toEqual(["protocol_error", "runtime_error", "runtime_error"]);
  } finally { await runner.close(); }
}, 30_000);

it("#12 shutdown waits for earlier accepted work before acknowledging", async () => {
  await raw([], async ({ send, wait, records }) => {
    send({ id: "earlier", source: "java.lang.Thread.sleep(200);42" });
    send({ id: "shutdown", op: "shutdown" });
    expect(await wait("earlier")).toMatchObject({ outcome: "ok", value: 42 });
    expect(await wait("shutdown")).toMatchObject({ outcome: "ok" });
    expect(records.map((record) => record.id)).toEqual(["earlier", "shutdown"]);
  });
});

it("#12 shutdown keeps routing replies, rejects later work and bounds its wait", async () => {
  await raw(["--shutdown-wait-ms=200"], async ({ send, wait, records }) => {
    send({ id: "blocked", engine: "idm", source: "openidm.read('blocked')", timeoutMs: 0 });
    await wait("blocked", true);
    send({ id: "shutdown", shutdown: true });
    send({ id: "later", source: "42" });
    expect(await wait("later")).toMatchObject({ outcome: "protocol_error", error: { message: "rhino-local: runner is shutting down" } });
    send({ type: "reply", id: "blocked", seq: 1, result: 42 });
    expect(await wait("blocked")).toMatchObject({ outcome: "ok", value: 42 });
    expect(await wait("shutdown")).toMatchObject({ outcome: "ok" });
    expect(records.findIndex((r) => r.id === "blocked" && "outcome" in r)).toBeLessThan(records.findIndex((r) => r.id === "shutdown"));
  });
  await raw(["--shutdown-wait-ms=100"], async ({ send, wait }) => {
    send({ id: "stuck", engine: "idm", source: "openidm.read('no-reply')", timeoutMs: 0 });
    await wait("stuck", true);
    send({ id: "shutdown", op: "shutdown" });
    expect(await wait("stuck")).toMatchObject({ outcome: "protocol_error", error: { message: "rhino-local: shutdown wait exceeded" } });
    expect(await wait("shutdown")).toMatchObject({ outcome: "ok" });
  });
});
