import { describe, it } from "vitest";
import { defineSuite, managed, useLease } from "../../../src/harness/index.ts";

// Invented fixture: example.com only, a placeholder UUID.
const ID = "00000000-0000-4000-8000-0000000000fa";
const SEND_DATE =
  'httpClient.send("https://service.example.com/x", { method: "POST", body: { d: new Date(0) } }).get();';

function suite(name: string, script: string) {
  return defineSuite({
    name,
    script: `${script}\naction.goTo("done");`,
    outcomes: ["done"],
    fixtures: {
      user: managed("managed/alpha_user", { _id: ID, userName: "fault-probe", mail: "fault-probe@example.com", givenName: "Fault", sn: "Probe" }),
    },
  });
}

/**
 * The cases each fixture file runs, in order. `refusal` catches a true refusal
 * (a JS Date in an httpClient.send body, harness fault script-defect/send-body-js-date) and
 * then succeeds; under E19 it must FAIL. `extra`, when given, is a second
 * caught refusal in the same case. `collide` catches AIC's own error for a
 * colliding create, which the harness emulates, and must PASS. `clean` runs
 * last on the same pooled JVM and must pass with nothing in the tally.
 */
export function faultFile(name: string, options: { twice?: boolean; extra?: string }): void {
  const refusal = suite(
    `${name} refusal`,
    [
      `try { ${SEND_DATE} } catch (e) { nodeState.putShared("caught", "yes"); }`,
      ...(options.twice ? [`try { ${SEND_DATE} } catch (e) {}`] : []),
      ...(options.extra ? [`try { ${options.extra} } catch (e) {}`] : []),
    ].join("\n"),
  );
  const collide = suite(
    `${name} collide`,
    `try { openidm.create("managed/alpha_user", "${ID}", { userName: "fault-probe-2" }); } catch (e) { nodeState.putShared("collided", String(e).indexOf("Entry Already Exists") >= 0 ? "yes" : String(e)); }`,
  );
  const clean = suite(`${name} clean`, 'nodeState.putShared("clean", "yes");');

  describe(`${name} refusal`, () => {
    const lease = useLease(refusal);
    it("caught refusal then success", async () => {
      await lease.run().expect({ outcome: "done", sharedState: { added: { caught: "yes" } }, http: [] });
    });
  });
  describe(`${name} collide`, () => {
    const lease = useLease(collide);
    it("caught emulated error", async () => {
      await lease.run().expect({
        outcome: "done",
        sharedState: { added: { collided: "yes" } },
        allowUndeclared: { openidmWrites: true },
      });
    });
  });
  describe(`${name} clean`, () => {
    const lease = useLease(clean);
    it("clean after a fault", async () => {
      await lease.run().expect({ outcome: "done", sharedState: { added: { clean: "yes" } } });
    });
  });
}
