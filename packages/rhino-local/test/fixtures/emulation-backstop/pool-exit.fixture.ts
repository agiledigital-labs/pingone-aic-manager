import { expect, inject, it } from "vitest";
import { JVM_POOL_CONTEXT } from "../../../src/pool.ts";
import { RhinoRunner } from "../../../src/runner.ts";
import { PID_PROBE } from "../../harness/isolation-scripts.ts";

let crashedPid: number;

it("loses two jobs on the JVM held by overlapping borrows", async () => {
  const address = inject(JVM_POOL_CONTEXT)!;
  const first = await RhinoRunner.borrow(address, { shared: true });
  const second = await RhinoRunner.borrow(address, { shared: true });
  try {
    crashedPid = Number((await first.eval(PID_PROBE)).value);
    expect(await second.eval(PID_PROBE)).toMatchObject({ value: crashedPid });
    const run = (runner: RhinoRunner, id: string) => runner.eval({
      id,
      source: `java.lang.System.err.println("${id}: ready"); for (;;) {}`,
      classAllowList: ["java.lang.System", "java.io.PrintStream"],
      timeoutMs: 0,
    });
    const outcomes = Promise.allSettled([run(first, "first-job"), run(second, "second-job")]);
    await expect.poll(() => first.stderr.includes("first-job: ready") && first.stderr.includes("second-job: ready"),
      { timeout: 15_000 }).toBe(true);
    process.kill(crashedPid, "SIGKILL");
    expect((await outcomes).map(({ status }) => status)).toEqual(["rejected", "rejected"]);
  } finally {
    await Promise.all([first.close(), second.close()]);
  }
}, 30_000);

it("the next borrow runs on a replacement JVM", async () => {
  const runner = await RhinoRunner.borrow(inject(JVM_POOL_CONTEXT)!, { shared: true });
  try {
    const response = await runner.eval(PID_PROBE);
    expect(response.outcome).toBe("ok");
    expect(response.value).not.toBe(crashedPid);
  } finally {
    await runner.close();
  }
});
