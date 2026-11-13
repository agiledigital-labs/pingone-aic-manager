import { describe, it } from "vitest";
import { defineSuite, useLease } from "../src/harness/index.ts";
import {
  benchLibraries,
  buildEsv,
  buildFixtures,
  buildHeaders,
  buildParams,
  expectedKeyCount,
  expectedN,
  isJourney,
  subjectScript,
  testsInFile,
} from "./workload.ts";

/**
 * One file of the decision-node bench. A journey test is two jobs (suspend,
 * then resume); every other test is one job.
 */
export function benchFile(file: number): void {
  const id = String(file).padStart(2, "0");
  const tests = testsInFile(file);
  describe(`decision file ${id}`, () => {
    const suite = defineSuite({
      name: `decision ${id}`,
      sourceName: `bench/decision-${id}.js`,
      script: subjectScript(),
      outcomes: ["done"],
      libraries: benchLibraries(),
      fixtures: buildFixtures(),
      always: {
        headers: buildHeaders(),
        params: buildParams(),
        esv: buildEsv(),
      },
    });
    const lease = useLease(suite);
    for (let test = 0; test < tests; test += 1) {
      if (isJourney(file, test)) {
        it(`journey ${test}`, async () => {
          await lease
            .run()
            .state({ shared: { ask: "yes" } })
            .step({
              expect: {
                callbacks: [{ type: "NameCallback", prompt: "Confirm" }],
                allowUndeclared: { sharedState: true },
              },
              reply: [{ type: "NameCallback", value: "ok" }],
            })
            .expect({
              outcome: "done",
              sharedState: {
                changed: { n: expectedN(true), keyCount: expectedKeyCount("journey-final") },
                added: { answer: "ok" },
              },
            });
        });
      } else {
        it(`case ${test}`, async () => {
          await lease.run().expect({
            outcome: "done",
            sharedState: {
              added: { n: expectedN(false), keyCount: expectedKeyCount("ordinary") },
            },
          });
        });
      }
    }
  });
}
