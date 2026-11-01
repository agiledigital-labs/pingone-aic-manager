/** Both-lane orchestration using TWO REAL HOST JVMs; Docker image compatibility is a separate gate. */
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

vi.mock("../src/jvm.ts", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../src/jvm.ts")>();
  return {
    ...actual,
    containerLaunch: (classes: string, jar: string) => {
      const launch = actual.hostLaunch(classes, jar);
      return {
        ...launch,
        args: ["-Drhino.local.replayShadow=true", ...launch.args],
      };
    },
  };
});
import { HostCallError, LaneDivergenceError, RhinoRunner } from "../src/runner.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn({ lane: "both" });
}, 60_000);
afterAll(async () => {
  await runner?.close();
}, 30_000);

const shadow = "java.lang.System.getProperty('rhino.local.replayShadow') !== null";

describe("IDM primary/shadow replay on two real host JVMs", () => {
  it("runs effects once, including a nested eval and thrown CREST reply", async () => {
    let effects = 0;
    const response = await runner.eval(
      {
        engine: "idm",
        source: "openidm.create('managed/example',null,{}).n+1",
      },
      {
        onCall: async () => {
          effects++;
          const nested = await runner.eval(
            {
              engine: "idm",
              source: "try{openidm.read('missing')}catch(e){e.javaException.getCode()}",
            },
            {
              onCall: () => {
                effects++;
                throw new HostCallError({
                  class: "org.forgerock.json.resource.NotFoundException",
                  message: "missing",
                  code: 404,
                  detail: {},
                });
              },
            }
          );
          expect(nested).toMatchObject({ outcome: "ok", value: 404 });
          return { n: 41 };
        },
      }
    );
    expect(response).toMatchObject({ outcome: "ok", value: 42 });
    expect(effects).toBe(2);
  });

  it.each([
    ["different method", `if(${shadow}) openidm.delete('x',null); else openidm.read('x'); 42`],
    ["different args", `openidm.read(${shadow}?'shadow':'primary'); 42`],
    ["extra call", `openidm.read('x'); if(${shadow}) {try{openidm.read('extra')}catch(e){}} 42`],
    ["missing call", `if(!(${shadow})) openidm.read('x'); 42`],
  ])(
    "raises LaneDivergenceError on %s even if the final response agrees",
    async (_label, source) => {
      let effects = 0;
      await expect(
        runner.eval(
          { engine: "idm", source },
          {
            onCall: () => {
              effects++;
              return null;
            },
          }
        )
      ).rejects.toBeInstanceOf(LaneDivergenceError);
      expect(effects).toBe(1);
    }
  );

  it("replays a host wait that times out without ever receiving a reply", async () => {
    let effects = 0;
    const response = await runner.eval(
      { engine: "idm", source: "openidm.read('wait')", timeoutMs: 100 },
      {
        onCall: () => {
          effects++;
          return new Promise(() => {});
        },
      }
    );
    expect(response.outcome).toBe("timeout");
    expect(effects).toBe(1);
  });

  it("snapshots handler results before later mutation", async () => {
    const reply = { n: 41 };
    const response = await runner.eval(
      {
        engine: "idm",
        source: "var a=openidm.read('first'); openidm.read('second'); a.n+1",
      },
      {
        onCall: (call) => {
          if (call.seq === 1) return reply;
          reply.n = 0;
          return null;
        },
      }
    );
    expect(response).toMatchObject({ outcome: "ok", value: 42 });
  });
});
