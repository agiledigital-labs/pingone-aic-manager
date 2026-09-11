import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner, RhinoRunnerExitError } from "../src/runner.ts";

describe("RhinoRunner", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  it("compiles and runs a trivial script", async () => {
    const response = await runner.eval({
      id: "ok-1",
      source: "1 + 1",
      sourceName: "trivial.js",
      timeoutMs: 2_000,
    });
    expect(response.id).toBe("ok-1");
    expect(response.outcome).toBe("ok");
    expect(response.value).toBe(2);
    expect(response.error).toBeNull();
  });

  it("injects JSON globals into ENGINE_SCOPE", async () => {
    const response = await runner.eval({
      source: "outcome + ':' + nodeState.foo",
      timeoutMs: 2_000,
      globals: { outcome: "true", nodeState: { foo: "bar" } },
    });
    expect(response.outcome).toBe("ok");
    expect(response.value).toBe("true:bar");
  });

  it("evals a preamble without shifting author line numbers", async () => {
    const response = await runner.eval({
      source: "greet()",
      sourceName: "author.js",
      preamble: 'function greet() { return "hi"; }',
      timeoutMs: 2_000,
    });
    expect(response.outcome).toBe("ok");
    expect(response.value).toBe("hi");
  });

  it("does not leak a global from one job into the next", async () => {
    const first = await runner.eval({
      id: "iso-1",
      source: 'leaked = "from-job-1"; var alsoLeaked = 1; function leakedFn() {} leaked',
      timeoutMs: 2_000,
    });
    expect(first.outcome).toBe("ok");
    expect(first.value).toBe("from-job-1");

    const second = await runner.eval({
      id: "iso-2",
      source: "[typeof leaked, typeof alsoLeaked, typeof leakedFn].join(',')",
      timeoutMs: 2_000,
    });
    expect(second.id).toBe("iso-2");
    expect(second.outcome).toBe("ok");
    expect(second.value).toBe("undefined,undefined,undefined");
  });

  it("does not leak Object.prototype mutations across jobs", async () => {
    const polluted = await runner.eval({
      source: "Object.prototype.leakedFromProto = 1; 1",
      timeoutMs: 2_000,
    });
    expect(polluted.outcome).toBe("ok");
    const next = await runner.eval({
      source: "typeof ({}.leakedFromProto)",
      timeoutMs: 2_000,
    });
    expect(next.outcome).toBe("ok");
    expect(next.value).toBe("undefined");
  });

  it("reports a parse error against the author's file and line", async () => {
    const dir = mkdtempSync(join(tmpdir(), "rhino-local-"));
    const sourceName = join(dir, "decision-node.cjs");
    writeFileSync(sourceName, "var ok = 1;\nlet banned = 2;\n");
    const response = await runner.eval({
      source: "var ok = 1;\nlet banned = 2;\n",
      sourceName,
      timeoutMs: 2_000,
    });
    expect(response.outcome).toBe("compile_error");
    expect(response.error).not.toBeNull();
    expect(response.error?.sourceName).toBe(sourceName);
    expect(response.error?.line).toBe(2);
    expect(response.error?.message).toContain("missing ; before statement");
    expect(response.error?.message).toContain(sourceName);
  });

  it("distinguishes runtime throw from compile failure", async () => {
    const response = await runner.eval({
      source: 'throw new Error("boom")',
      sourceName: "throw.js",
      timeoutMs: 2_000,
    });
    expect(response.outcome).toBe("runtime_error");
    expect(response.error?.class).toBe("org.mozilla.javascript.JavaScriptException");
    expect(response.error?.sourceName).toBe("throw.js");
    expect(response.error?.line).toBe(1);
  });

  it("times out a runaway loop per-job and keeps serving", async () => {
    const timed = await runner.eval({
      id: "loop",
      source: "while (true) {}",
      sourceName: "loop.js",
      timeoutMs: 200,
    });
    expect(timed.outcome).toBe("timeout");
    expect(timed.error?.message).toBe("Interrupt.");
    const after = await runner.eval({
      id: "after-loop",
      source: "2 + 2",
      timeoutMs: 2_000,
    });
    expect(after.id).toBe("after-loop");
    expect(after.outcome).toBe("ok");
    expect(after.value).toBe(4);
  }, 15_000);

  it("correlates concurrent jobs by id, not arrival order", async () => {
    const [a, b, c] = await Promise.all([
      runner.eval({ id: "conc-a", source: "'A'", timeoutMs: 2_000 }),
      runner.eval({ id: "conc-b", source: "'B'", timeoutMs: 2_000 }),
      runner.eval({ id: "conc-c", source: "'C'", timeoutMs: 2_000 }),
    ]);
    expect(a).toMatchObject({ id: "conc-a", outcome: "ok", value: "A" });
    expect(b).toMatchObject({ id: "conc-b", outcome: "ok", value: "B" });
    expect(c).toMatchObject({ id: "conc-c", outcome: "ok", value: "C" });
  });

  it("matches AIC on top-level const (decision-node scope)", async () => {
    const response = await runner.eval({
      source: 'const TOP = "const-top-level-ok"; String(TOP)',
      sourceName: "const-top-level.js",
      timeoutMs: 2_000,
    });
    expect(response.outcome).toBe("ok");
    expect(response.value).toBe("undefined");
  });
});

describe("RhinoRunner crash", () => {
  it("rejects every pending job when the JVM exits", async () => {
    const runner = await RhinoRunner.spawn();
    try {
      const pending = runner.eval({
        source: "while (true) {}",
        timeoutMs: 60_000,
      });
      const rejected = expect(pending).rejects.toBeInstanceOf(RhinoRunnerExitError);
      await runner.kill();
      await rejected;
    } finally {
      await runner.close();
    }
  }, 60_000);
});
