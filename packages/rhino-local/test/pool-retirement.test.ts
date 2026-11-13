import { describe, expect, it } from "vitest";
import { RetirementWatch } from "../src/pool.ts";

const MARKER = "rhino-local-runner retired";

describe("RetirementWatch", () => {
  it("sees a marker split across chunks", () => {
    const watch = new RetirementWatch();
    watch.feed("some script output\nrhino-local-runner ret");
    expect(watch.retired).toBe(false);
    watch.feed("ired\nmore output\n");
    expect(watch.retired).toBe(true);
  });

  it("sees a marker delivered one character at a time", () => {
    const watch = new RetirementWatch();
    for (const char of `noise\n${MARKER}\n`) watch.feed(char);
    expect(watch.retired).toBe(true);
  });

  it("sees a marker after a long unterminated line", () => {
    const watch = new RetirementWatch();
    watch.feed("x".repeat(100_000));
    watch.feed(`\n${MARKER}\n`);
    expect(watch.retired).toBe(true);
  });

  it("does not fire on a partial marker", () => {
    const watch = new RetirementWatch();
    watch.feed("rhino-local-runner ret\nired\n");
    expect(watch.retired).toBe(false);
  });
});
