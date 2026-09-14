import { afterAll, describe, expect, it } from "vitest";
import { z } from "zod";
import { HARNESS_CALLBACK_ID } from "../../src/aic/constants.ts";
import type { HttpRequest, HttpResponse } from "../../src/aic/http.ts";
import type { AicIo } from "../../src/aic/tenant.ts";
import { defineSuite, useLease } from "../../src/harness/index.ts";

const SOURCE = 'action.goTo("done");\n';

const suite = defineSuite({
  name: "vitest-aic-adapter",
  script: SOURCE,
  outcomes: ["done"],
  inputs: z.object({ note: z.string() }),
  cleanup: async (idm) => {
    await idm.read("managed/alpha_user/local-only-cleanup");
  },
});

describe("useLease AIC adapter", () => {
  const fake = new AdapterAicIo();
  // Vitest unwinds afterAll hooks in stack order. Register the assertion
  // before useLease so it observes the adapter's teardown rather than racing
  // it.
  afterAll(() => {
    const firstCreate = fake.events.indexOf("static-create");
    const authenticate = fake.events.indexOf("authenticate");
    const firstDelete = fake.events.indexOf("delete");
    expect(firstCreate).toBeGreaterThanOrEqual(0);
    expect(authenticate).toBeGreaterThan(firstCreate);
    expect(firstDelete).toBeGreaterThan(authenticate);
  });
  const lease = useLease(suite, {
    aic: { id: "vitest-aic-adapter", realm: "bravo" },
    aicIo: fake.io,
    timeoutMs: 10_000,
  });

  it("checks the completed local run through the pre-opened AIC file lease", async () => {
    const run = await lease
      .run({ note: "not-secret" })
      .check(async (idm) => {
        await idm.read("managed/alpha_user/local-only-check");
      })
      .expect({ outcome: "done" });

    expect(run.conformance?.passes).toHaveLength(1);
    expect(run.kase.given.realm).toBe("bravo");
    expect(run.conformance?.passes[0]?.aic.verdict?.pass).toBe(true);
    expect(run.conformance?.observationGaps).toContainEqual(
      expect.objectContaining({
        channel: "openidm",
        path: "checks/cleanup",
        local: "local IdmHandle only",
        aic: "not replayed",
      })
    );
    expect(fake.events).toEqual(
      expect.arrayContaining(["static-create", "arm", "authenticate"])
    );
  });

});

describe("useLease without AIC opt-in", () => {
  const fake = new AdapterAicIo();
  const lease = useLease(suite, {
    // A test seam alone must not opt a file into tenant work.
    aicIo: fake.io,
    timeoutMs: 10_000,
  });

  it("makes no AIC calls", async () => {
    const run = await lease.run({ note: "local" }).expect({ outcome: "done" });
    expect(run.conformance).toBeUndefined();
    expect(fake.calls).toBe(0);
  });
});

class AdapterAicIo {
  readonly events: string[] = [];
  calls = 0;
  readonly #resources = new Map<string, Record<string, unknown>>();
  #nonce = "";
  #subjectDigest = "";
  #leaseDigest = "";

  readonly io: AicIo = {
    aic: (args) => {
      this.calls += 1;
      if (args.includes("ctx") && args.includes("list")) {
        return Promise.resolve({
          status: 0,
          stdout: JSON.stringify([
            {
              current: true,
              name: "test-context",
              base_url: "https://tenant.invalid",
            },
          ]),
          stderr: "",
        });
      }
      return Promise.resolve({ status: 0, stdout: "test-token\n", stderr: "" });
    },
    http: (request) => this.#http(request),
  };

  #http(request: HttpRequest): Promise<HttpResponse> {
    this.calls += 1;
    const url = new URL(request.url);
    const path = url.pathname;
    if (request.method === "GET") {
      return Promise.resolve(
        this.#resources.has(path)
          ? json(200, this.#resources.get(path))
          : json(404, { code: 404 })
      );
    }
    if (request.method === "PUT") {
      const body = JSON.parse(String(request.body)) as Record<string, unknown>;
      const exists = this.#resources.has(path);
      if (exists && isSubjectScript(path, body)) {
        this.events.push("arm");
        const source = decodeSource(body);
        this.#nonce = capture(source, /invocationNonce: "([^"]+)"/);
        this.#subjectDigest = capture(source, /subjectDigest: "([0-9a-f]+)"/);
      } else {
        this.events.push("static-create");
        if (path.includes("/scripts/") && String(body.name).includes("-result-")) {
          this.#leaseDigest = capture(
            decodeSource(body),
            /leaseDigest: "([0-9a-f]+)"/
          );
        }
      }
      this.#resources.set(path, readBack(path, body));
      return Promise.resolve(json(exists ? 200 : 201, {}));
    }
    if (request.method === "DELETE") {
      this.events.push("delete");
      this.#resources.delete(path);
      return Promise.resolve(json(200, {}));
    }
    this.events.push("authenticate");
    return Promise.resolve(
      json(200, {
        callbacks: [
          {
            type: "HiddenValueCallback",
            output: [
              { name: "id", value: HARNESS_CALLBACK_ID },
              {
                name: "value",
                value: JSON.stringify({
                  outcome: "done",
                  before: { note: "not-secret" },
                  final: { note: "not-secret" },
                  invocationNonce: this.#nonce,
                  subjectDigest: this.#subjectDigest,
                  leaseDigest: this.#leaseDigest,
                }),
              },
            ],
          },
        ],
      })
    );
  }
}

function isSubjectScript(path: string, body: Record<string, unknown>): boolean {
  return path.includes("/scripts/") && String(body.name).endsWith("-subject");
}

function decodeSource(body: Record<string, unknown>): string {
  return Buffer.from(String(body.script), "base64").toString("utf8");
}

function capture(source: string, pattern: RegExp): string {
  return pattern.exec(source)?.[1] ?? "";
}

function readBack(
  path: string,
  body: Record<string, unknown>
): Record<string, unknown> {
  if (path.includes("/scripts/")) {
    return {
      ...body,
      createdBy: "server",
      creationDate: 1,
      lastModifiedBy: "server",
      lastModifiedDate: 1,
    };
  }
  if (path.includes("/nodes/ScriptedDecisionNode/")) {
    return { ...body, _id: "server", _rev: "rev", _type: {}, _outcomes: [] };
  }
  const nodes = body.nodes as Record<string, Record<string, unknown>>;
  return {
    ...body,
    _id: "server",
    _rev: "rev",
    innerTreeOnly: false,
    noSession: false,
    mustRun: false,
    transactionalOnly: false,
    nodes: Object.fromEntries(
      Object.entries(nodes).map(([id, node]) => [
        id,
        { ...node, version: "1.0" },
      ])
    ),
  };
}

function json(status: number, body: unknown): HttpResponse {
  return { status, headers: [], body: JSON.stringify(body) };
}
