/**
 * List failed AIC-lane tests and fetch their logs.
 *
 *   npm run show-log
 *
 * Requires an unlocked agent (`aic login`). Fetches with `aic logs tx` only —
 * never a range query.
 */
import { createDefaultIo, runShowLog } from "../src/harness/show-log.ts";

try {
  const status = await runShowLog(createDefaultIo());
  process.exitCode = status;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
