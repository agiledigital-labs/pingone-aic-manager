import { describe, it } from "vitest";
import { defineSuite, useLease } from "../../../src/harness/index.ts";
import { faultCase, SEND_DATE } from "./cases.ts";

faultCase("timeout date", SEND_DATE);
// The test times out with its case still running on the JVM: the work is
// outstanding at the test's end, which the audit records.
const spin = defineSuite({ name: "timeout spin", script: 'var t = Date.now(); while (Date.now() - t < 3000) {}\naction.goTo("done");', outcomes: ["done"] });
describe("timeout spin", () => {
  const lease = useLease(spin);
  it("times out", { timeout: 300 }, async () => {
    await lease.run().expect({ outcome: "done" });
  });
});
