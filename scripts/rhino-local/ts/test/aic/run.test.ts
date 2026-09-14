import { describe, expect, it, vi } from "vitest";
import { HARNESS_CALLBACK_ID } from "../../src/aic/constants.ts";
import type { HttpRequest, HttpResponse } from "../../src/aic/http.ts";
import { runAicLane } from "../../src/aic/run.ts";
import { AicLaneError, type AicIo, type CliResult } from "../../src/aic/tenant.ts";
import { caseWith } from "./helpers.ts";

const PLACEHOLDER_BASE = "https://tenant.example.com";
const SUBJECT = 'nodeState.putShared("verified", true);\naction.goTo("true");\n';
const MANAGED_FIXTURE = {
  type: "managed/alpha_user",
  record: { _id: "fixture-alice", userName: "alice" },
} as const;

describe("runAicLane", () => {
  it("creates namespaced resources, invokes authenticate, records effects, and deletes", async () => {
    const fake = mockTenant();
    const kase = caseWith({
      given: {
        sharedState: { username: "alice" },
        requestHeaders: { "x-aic-probe": ["alpha", "bravo"] },
        requestParameters: { probeq: ["one"] },
      },
      expect: { outcome: "true", sharedState: { added: { verified: true } } },
    });
    const effects = await runAicLane(kase, SUBJECT, {
      io: fake.io,
      runId: "test01",
      project: "/tmp/rhino-local-aic-test",
    });
    expect(effects.outcome).toBe("true");
    expect(effects.sharedState.final).toEqual({ username: "alice" });
    expect(effects.evidence?.unbucketedState).toEqual([
      {
        operation: "added",
        key: "verified",
        after: true,
        possibleBuckets: ["sharedState", "transientState"],
      },
    ]);
    expect(effects.evidence?.unobservedChannels).toEqual([
      "openidm",
      "http",
      "logs",
    ]);
    expect(effects.openidm).toEqual([]);
    expect(fake.aicArgs.every((args) => args[0] === "--no-prompt")).toBe(true);
    expect(fake.aicArgs.some((args) => args.includes("whoami") && args.includes("--token"))).toBe(
      true
    );

    const puts = fake.httpCalls.filter((call) => call.method === "PUT");
    const deletes = fake.httpCalls.filter((call) => call.method === "DELETE");
    const authenticate = fake.httpCalls.find((call) => call.method === "POST");
    expect(puts.some((call) => call.url.includes("/trees/rl-aic-test01"))).toBe(true);
    expect(puts.some((call) => call.url.includes("/scripts/"))).toBe(true);
    expect(deletes.length).toBe(puts.length);
    expect(deletes.some((call) => call.url.includes("/trees/rl-aic-test01"))).toBe(true);
    expect(authenticate?.url).toContain("authIndexType=service");
    expect(authenticate?.url).toContain("authIndexValue=rl-aic-test01");
    expect(authenticate?.url).toContain("probeq=one");
    expect(authenticate?.headerLines.filter(([name]) => name.toLowerCase() === "x-aic-probe")).toEqual(
      [
        ["x-aic-probe", "alpha"],
        ["x-aic-probe", "bravo"],
      ]
    );
    expect(authenticate?.headerLines.some(([name]) => name === "Authorization")).toBe(false);
  });

  it("refuses to overwrite an existing tree", async () => {
    const fake = mockTenant({ existing: ["/am/json/realms/root/realms/alpha/realm-config/authentication/authenticationtrees/trees/rl-aic-test01"] });
    await expect(
      runAicLane(caseWith(), SUBJECT, {
        io: fake.io,
        runId: "test01",
        project: "/tmp/rhino-local-aic-test",
      })
    ).rejects.toBeInstanceOf(AicLaneError);
    expect(fake.httpCalls.filter((call) => call.method === "PUT")).toEqual([]);
  });

  it("cleans up scripts and nodes if authenticate fails", async () => {
    const fake = mockTenant({ authenticateStatus: 401 });
    await expect(
      runAicLane(caseWith(), SUBJECT, {
        io: fake.io,
        runId: "boom01",
        project: "/tmp/rhino-local-aic-test",
      })
    ).rejects.toThrow(/authenticate HTTP 401/);
    const puts = fake.httpCalls.filter((call) => call.method === "PUT");
    const deletes = fake.httpCalls.filter((call) => call.method === "DELETE");
    expect(puts.length).toBeGreaterThan(0);
    expect(deletes.length).toBe(puts.length);
  });

  it("does not send the tenant hostname in errors", async () => {
    const fake = mockTenant({ authenticateStatus: 401, authenticateBody: { message: `${PLACEHOLDER_BASE} no` } });
    try {
      await runAicLane(caseWith(), SUBJECT, {
        io: fake.io,
        runId: "redact",
        project: "/tmp/rhino-local-aic-test",
      });
      throw new Error("expected AicLaneError");
    } catch (error) {
      expect(error).toBeInstanceOf(AicLaneError);
      expect(String(error)).not.toContain(PLACEHOLDER_BASE);
      expect(String(error)).toContain("<redacted>");
    }
  });

  it("creates and verifies a managed fixture before the subject, then deletes it", async () => {
    const fake = mockTenant();
    await runAicLane(managedCase(), SUBJECT, {
      io: fake.io,
      runId: "managed1",
      project: "/tmp/rhino-local-aic-test",
      managedFixtures: [MANAGED_FIXTURE],
    });

    const create = fake.httpCalls.findIndex(
      (call) =>
        call.method === "POST" && call.url.includes("?_action=create")
    );
    const check = fake.httpCalls.findIndex(
      (call) =>
        call.method === "GET" && call.url.includes("/fixture-alice")
    );
    const subject = fake.httpCalls.findIndex(
      (call) => call.method === "POST" && call.url.includes("/authenticate")
    );
    const remove = fake.httpCalls.findIndex(
      (call) =>
        call.method === "DELETE" && call.url.includes("/fixture-alice")
    );
    expect(create).toBeGreaterThanOrEqual(0);
    expect(check).toBeGreaterThan(create);
    expect(subject).toBeGreaterThan(check);
    expect(remove).toBeGreaterThan(subject);
    expect(JSON.parse(String(fake.httpCalls[create]?.body))).toEqual(
      MANAGED_FIXTURE.record
    );
  });

  it("refuses a managed fixture collision and never invokes the subject", async () => {
    const fake = mockTenant({ managedCreateStatus: 412 });
    await expect(
      runAicLane(managedCase(), SUBJECT, {
        io: fake.io,
        runId: "managed2",
        project: "/tmp/rhino-local-aic-test",
        managedFixtures: [MANAGED_FIXTURE],
      })
    ).rejects.toThrow(/collision.*fixture-alice/);
    expect(
      fake.httpCalls.some(
        (call) => call.method === "POST" && call.url.includes("/authenticate")
      )
    ).toBe(false);
  });

  it("anti-silencing: refuses a fixture the create response did not make readable", async () => {
    const fake = mockTenant({ managedReadStatus: 404 });
    await expect(
      runAicLane(managedCase(), SUBJECT, {
        io: fake.io,
        runId: "managed3",
        project: "/tmp/rhino-local-aic-test",
        managedFixtures: [MANAGED_FIXTURE],
      })
    ).rejects.toThrow(/fixture-alice.*not readable after create/);
    expect(
      fake.httpCalls.some(
        (call) => call.method === "POST" && call.url.includes("/authenticate")
      )
    ).toBe(false);
    expect(
      fake.httpCalls.some(
        (call) => call.method === "DELETE" && call.url.includes("/fixture-alice")
      )
    ).toBe(true);
  });

  it("deletes managed fixtures when the subject throws", async () => {
    const fake = mockTenant({ authenticateStatus: 401 });
    await expect(
      runAicLane(managedCase(), SUBJECT, {
        io: fake.io,
        runId: "managed4",
        project: "/tmp/rhino-local-aic-test",
        managedFixtures: [MANAGED_FIXTURE],
      })
    ).rejects.toThrow(/authenticate HTTP 401/);
    const subject = fake.httpCalls.findIndex(
      (call) => call.method === "POST" && call.url.includes("/authenticate")
    );
    const remove = fake.httpCalls.findIndex(
      (call) =>
        call.method === "DELETE" && call.url.includes("/fixture-alice")
    );
    expect(remove).toBeGreaterThan(subject);
  });

  it("reports every managed fixture it could not delete", async () => {
    const fake = mockTenant({ managedDeleteStatus: 500 });
    const warning = vi
      .spyOn(process, "emitWarning")
      .mockImplementation(() => undefined);
    try {
      await runAicLane(managedCase(), SUBJECT, {
        io: fake.io,
        runId: "managed5",
        project: "/tmp/rhino-local-aic-test",
        managedFixtures: [MANAGED_FIXTURE],
      });
      expect(warning).toHaveBeenCalledWith(
        expect.stringMatching(/fixture-alice.*HTTP 500/)
      );
    } finally {
      warning.mockRestore();
    }
  });
});

function managedCase() {
  return caseWith({
    given: {
      managed: {
        [MANAGED_FIXTURE.type]: [MANAGED_FIXTURE.record],
      },
    },
  });
}

function mockTenant(
  options: {
    existing?: string[];
    authenticateStatus?: number;
    authenticateBody?: unknown;
    managedCreateStatus?: number;
    managedReadStatus?: number;
    managedDeleteStatus?: number;
  } = {}
): { io: AicIo; httpCalls: HttpRequest[]; aicArgs: string[][] } {
  const existing = new Set(options.existing ?? []);
  const httpCalls: HttpRequest[] = [];
  const aicArgs: string[][] = [];
  const dump = JSON.stringify({
    outcome: "true",
    before: { username: "alice" },
    final: { username: "alice", verified: true },
  });
  const authenticateBody = options.authenticateBody ?? {
    callbacks: [
      {
        type: "HiddenValueCallback",
        output: [
          { name: "id", value: HARNESS_CALLBACK_ID },
          { name: "value", value: dump },
        ],
      },
    ],
  };
  let managedRecord: unknown;

  const io: AicIo = {
    async aic(args) {
      aicArgs.push(args);
      if (args.includes("ctx") && args.includes("list")) {
        return ok(
          JSON.stringify([
            {
              current: true,
              name: "sandbox",
              theme: "sandbox",
              base_url: PLACEHOLDER_BASE,
            },
          ])
        );
      }
      if (args.includes("whoami")) {
        return ok("test-token\n");
      }
      return { status: 1, stdout: "", stderr: `unexpected aic ${args.join(" ")}` };
    },
    async http(req) {
      httpCalls.push(req);
      const url = new URL(req.url);
      const path = url.pathname;
      if (req.method === "GET") {
        if (path.startsWith("/openidm/managed/")) {
          const status = options.managedReadStatus ?? (managedRecord === undefined ? 404 : 200);
          return json(status, status === 200 ? managedRecord : { code: status });
        }
        if (existing.has(path)) {
          return json(200, { _id: "already-there" });
        }
        return json(404, { code: 404 });
      }
      if (req.method === "PUT") {
        return json(201, { _id: "created" });
      }
      if (req.method === "DELETE") {
        if (path.startsWith("/openidm/managed/")) {
          managedRecord = undefined;
          return json(options.managedDeleteStatus ?? 200, {});
        }
        return json(200, {});
      }
      if (
        req.method === "POST" &&
        path.startsWith("/openidm/managed/") &&
        url.searchParams.get("_action") === "create"
      ) {
        const status = options.managedCreateStatus ?? 201;
        if (status === 201) {
          managedRecord = JSON.parse(String(req.body));
        }
        return json(status, status === 201 ? managedRecord : { code: status });
      }
      if (req.method === "POST" && path.endsWith("/authenticate")) {
        return json(options.authenticateStatus ?? 200, authenticateBody);
      }
      return json(500, { message: `unexpected ${req.method} ${path}` });
    },
  };
  return { io, httpCalls, aicArgs };
}

function ok(stdout: string): CliResult {
  return { status: 0, stdout, stderr: "" };
}

function json(status: number, body: unknown): HttpResponse {
  return {
    status,
    headers: [["Content-Type", "application/json"]],
    body: JSON.stringify(body),
  };
}
