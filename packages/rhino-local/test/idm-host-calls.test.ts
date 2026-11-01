/** Re-entrant integration tests: every eval goes through the live JVM reader/worker protocol. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner, type HostCall } from "../src/runner.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
}, 30_000);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("IDM re-entrant host calls", () => {
  it("nests three IDM jobs on one JVM while each caller is blocked", async () => {
    const trace: string[] = [];
    const outer = await runner.eval(
      {
        engine: "idm",
        source: "openidm.create('managed/example',null,{a:1}).answer+1",
      },
      {
        onCall: async (call) => {
          trace.push(call.method);
          const middle = await runner.eval(
            {
              engine: "idm",
              source: "openidm.update('managed/example/1',null,{a:2}).answer+1",
            },
            {
              onCall: async (middleCall) => {
                trace.push(middleCall.method);
                const inner = await runner.eval(
                  {
                    engine: "idm",
                    source: "openidm.read('managed/example/1').answer+1",
                  },
                  {
                    onCall: (innerCall) => {
                      trace.push(innerCall.method);
                      return { answer: 39 };
                    },
                  }
                );
                expect(inner).toMatchObject({ outcome: "ok", value: 40 });
                return { answer: inner.value };
              },
            }
          );
          expect(middle).toMatchObject({ outcome: "ok", value: 41 });
          return { answer: middle.value };
        },
      }
    );
    expect(outer).toMatchObject({ outcome: "ok", value: 42 });
    expect(trace).toEqual(["create", "update", "read"]);
  });

  it("two independent outer jobs each nest, with replies arriving interleaved", async () => {
    const leafA = deferred<void>();
    const leafB = deferred<void>();
    const releaseA = deferred<void>();
    const releaseB = deferred<void>();
    const replyOrder: string[] = [];
    const run = (
      name: string,
      started: ReturnType<typeof deferred<void>>,
      release: ReturnType<typeof deferred<void>>
    ) =>
      runner.eval(
        {
          id: `outer-${name}`,
          engine: "idm",
          source: `openidm.read('${name}').answer`,
        },
        {
          onCall: async () => {
            const nested = await runner.eval(
              {
                id: `inner-${name}`,
                engine: "idm",
                source: `openidm.read('${name}-leaf').answer`,
              },
              {
                onCall: async () => {
                  started.resolve();
                  await release.promise;
                  replyOrder.push(name);
                  return { answer: name };
                },
              }
            );
            expect(nested.outcome).toBe("ok");
            return { answer: nested.value };
          },
        }
      );
    const a = run("A", leafA, releaseA);
    const b = run("B", leafB, releaseB);
    await Promise.all([leafA.promise, leafB.promise]);
    releaseB.resolve();
    expect(await b).toMatchObject({ id: "outer-B", outcome: "ok", value: "B" });
    releaseA.resolve();
    expect(await a).toMatchObject({ id: "outer-A", outcome: "ok", value: "A" });
    expect(replyOrder).toEqual(["B", "A"]);
  });

  it("AM completes while an IDM job is blocked on a call", async () => {
    const started = deferred<void>();
    const release = deferred<void>();
    const blocked = runner.eval(
      { engine: "idm", source: "openidm.read('wait').answer" },
      {
        onCall: async () => {
          started.resolve();
          await release.promise;
          return { answer: 42 };
        },
      }
    );
    await started.promise;
    try {
      expect(await runner.eval({ source: "21*2" })).toMatchObject({
        outcome: "ok",
        value: 42,
      });
    } finally {
      release.resolve();
    }
    expect(await blocked).toMatchObject({ outcome: "ok", value: 42 });
  });

  it("sequence numbers are one-based per job and handler throws are catchable", async () => {
    const calls: HostCall[] = [];
    const response = await runner.eval(
      {
        engine: "idm",
        source: `var x=openidm.read('a');
      try {openidm.read('b')}catch(e){x+=e.message.indexOf('handler failed')>=0?1:0} x+openidm.read('c')`,
      },
      {
        onCall: (call) => {
          calls.push(call);
          if (call.seq === 2) throw new Error("handler failed");
          return call.seq === 1 ? 40 : 1;
        },
      }
    );
    expect(response).toMatchObject({ outcome: "ok", value: 42 });
    expect(calls.map((c) => c.seq)).toEqual([1, 2, 3]);
    expect(new Set(calls.map((c) => c.id))).toEqual(new Set([response.id]));
  });

  it("query callback can issue a host call whose handler runs another script", async () => {
    const response = await runner.eval(
      {
        engine: "idm",
        source: `var sum=0;
      openidm.query('managed/example',{_queryFilter:'true'},function(row){sum+=openidm.read(row.id).n}); sum`,
      },
      {
        onCall: async (call) => {
          if (call.method === "query")
            return { result: [{ id: "a" }, { id: "b" }], resultCount: 2 };
          const inner = await runner.eval({ engine: "idm", source: "21" });
          return { n: inner.value };
        },
      }
    );
    expect(response).toMatchObject({ outcome: "ok", value: 42 });
  });

  it("a job without onCall fails the call immediately instead of hanging", async () => {
    const response = await runner.eval({
      engine: "idm",
      timeoutMs: 0,
      source: "openidm.read('x')",
    });
    expect(response.outcome).toBe("runtime_error");
    expect(response.thrown).toMatchObject({
      kind: "javaException",
      message: "rhino-local: no onCall handler for IDM job",
    });
  });

  it("times out a blocked host call and ignores its late reply while other jobs survive", async () => {
    const started = deferred<void>();
    const release = deferred<void>();
    const blocked = runner.eval(
      {
        id: "late-reply",
        engine: "idm",
        timeoutMs: 100,
        source: "openidm.read('wait')",
      },
      {
        onCall: async () => {
          started.resolve();
          await release.promise;
          return 1;
        },
      }
    );
    await started.promise;
    expect((await blocked).outcome).toBe("timeout");
    release.resolve();
    expect(await runner.eval({ id: "late-reply", engine: "idm", source: "42" })).toMatchObject({
      outcome: "ok",
      value: 42,
    });
  });

  it("rejects duplicate active job ids without replacing the pending handler", async () => {
    const started = deferred<void>();
    const release = deferred<void>();
    const first = runner.eval(
      { id: "duplicate", engine: "idm", source: "openidm.read('wait')" },
      {
        onCall: async () => {
          started.resolve();
          await release.promise;
          return 42;
        },
      }
    );
    await started.promise;
    try {
      await expect(runner.eval({ id: "duplicate", source: "1" })).rejects.toThrow(
        "duplicate active job id"
      );
    } finally {
      release.resolve();
    }
    expect(await first).toMatchObject({ outcome: "ok", value: 42 });
  });

  it("keeps logs with a failing job and binding mutations before its throw", async () => {
    const response = await runner.eval({
      engine: "idm",
      bindings: { object: {} },
      source: "object.changed=true; logger.info('before'); throw 'failed'",
    });
    expect(response.outcome).toBe("runtime_error");
    expect(response.bindings).toEqual({ object: { changed: true } });
    expect(response.logs?.map((log) => log.message)).toEqual(["before"]);
  });
});


it("#6 allocates distinct host sequences for simultaneous shell spawn calls", async () => {
  for (let batch = 0; batch < 30; batch++) {
    const sequences: number[] = [];
    const response = await runner.eval({ engine: "idm", timeoutMs: 1500, source: `
      var ready=new java.util.concurrent.CountDownLatch(32), start=new java.util.concurrent.CountDownLatch(1);
      var threads=[];
      for(var i=0;i<32;i++) threads.push(spawn(function(){ready.countDown();start.await();openidm.read('spawned');}));
      ready.await(); start.countDown();
      for(var j=0;j<threads.length;j++) threads[j].join(); 42;
    ` }, { onCall: (call) => { sequences.push(call.seq); return 42; } });
    expect(new Set(sequences).size, `batch ${batch}: ${sequences.join()}`).toBe(32);
    expect([...sequences].sort((a,b) => a-b)).toEqual(Array.from({length:32}, (_,i) => i+1));
    expect(response.outcome).toBe("ok");
  }
}, 60_000);
