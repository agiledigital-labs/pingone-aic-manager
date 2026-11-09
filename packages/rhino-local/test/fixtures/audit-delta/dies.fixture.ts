import { it } from "vitest";
import { faultCase, SEND_DATE } from "./cases.ts";

// The fault is recorded and sent; then the worker process dies outright,
// before its file finishes, so nothing after this point is ever sent.
faultCase("dies date", SEND_DATE);
it("kills its worker", async () => {
  await new Promise((resolve) => setTimeout(resolve, 300));
  process.kill(process.pid, "SIGKILL");
});
