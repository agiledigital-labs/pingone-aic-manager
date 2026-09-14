import { afterEach, describe, expect, it, vi } from "vitest";
import { HARNESS_CALLBACK_ID } from "../../src/aic/constants.ts";
import { headerValues, type HttpRequest, type HttpResponse } from "../../src/aic/http.ts";
import type { ManagedFixture } from "../../src/aic/managed.ts";
import { runAicLane } from "../../src/aic/run.ts";
import { AicLaneError, type AicIo, type CliResult } from "../../src/aic/tenant.ts";
import { clearAicTrace, peekAicTrace } from "../../src/aic/trace.ts";
import { TX_HEADER } from "../../src/aic/txid.ts";
import { caseWith } from "./helpers.ts";

afterEach(() => {
  clearAicTrace();
});

const PLACEHOLDER_BASE = "https://tenant.example.com";
const SUBJECT = 'nodeState.putShared("verified", true);\naction.goTo("true");\n';
const MANAGED_ID = "00000000-0000-4000-8000-000000000001";
const MANAGED_FIXTURE: ManagedFixture = {
  type: "managed/alpha_user",
  record: { _id: MANAGED_ID, userName: "alice" },
};

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
    const firstPut = fake.httpCalls.findIndex((call) => call.method === "PUT");
    expect(fake.httpCalls.slice(0, firstPut).every((call) => call.method === "GET")).toBe(true);
    for (const put of puts) {
      const index = fake.httpCalls.indexOf(put);
      expect(fake.httpCalls[index + 1]?.method).toBe("GET");
      expect(fake.httpCalls[index + 1]?.url).toBe(put.url);
    }
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
    const sent = headerValues(authenticate?.headerLines ?? [], TX_HEADER);
    expect(sent).toHaveLength(1);
    expect(sent[0]).toMatch(/-01$/);
    expect(peekAicTrace()?.passIds).toEqual(sent);
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

  it("refuses server-normalized script source without printing it", async () => {
    const fake = mockTenant({ normalizedScriptSource: "server changed source" });
    await expect(
      runAicLane(caseWith(), SUBJECT, {
        io: fake.io,
        runId: "normalized",
        project: "/tmp/rhino-local-aic-test",
      })
    ).rejects.toThrow(/confirming read did not match/);
    try {
      await runAicLane(caseWith(), SUBJECT, {
        io: mockTenant({ normalizedScriptSource: "server changed source" }).io,
        runId: "normalized-secret",
        project: "/tmp/rhino-local-aic-test",
      });
    } catch (error) {
      expect(String(error)).not.toContain(SUBJECT.trim());
      expect(String(error)).not.toContain("server changed source");
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
        call.method === "GET" && call.url.includes(`/${MANAGED_ID}`)
    );
    const subject = fake.httpCalls.findIndex(
      (call) => call.method === "POST" && call.url.includes("/authenticate")
    );
    const remove = fake.httpCalls.findIndex(
      (call) =>
        call.method === "DELETE" && call.url.includes(`/${MANAGED_ID}`)
    );
    expect(create).toBeGreaterThanOrEqual(0);
    expect(check).toBeGreaterThan(create);
    expect(subject).toBeGreaterThan(check);
    expect(remove).toBeGreaterThan(subject);
    expect(JSON.parse(String(fake.httpCalls[create]?.body))).toEqual(
      MANAGED_FIXTURE.record
    );
  });

  it("accepts an alpha_user fixture when GET omits its write-only password", async () => {
    const fake = mockTenant({ managedReadOmit: ["password"] });
    const fixture: ManagedFixture = {
      type: "managed/alpha_user",
      record: {
        _id: "00000000-0000-4000-8000-000000000002",
        userName: "password-fixture",
        password: "Test-only password 1!",
      },
    };

    await runAicLane(managedCase(fixture), SUBJECT, {
      io: fake.io,
      runId: "managed-password",
      project: "/tmp/rhino-local-aic-test",
      managedFixtures: [fixture],
    });

    expect(
      fake.httpCalls.some(
        (call) => call.method === "POST" && call.url.includes("/authenticate")
      )
    ).toBe(true);
  });

  it("anti-silencing: refuses a fixture whose readable field did not land", async () => {
    const fake = mockTenant({ managedReadOmit: ["userName"] });
    await expect(
      runAicLane(managedCase(), SUBJECT, {
        io: fake.io,
        runId: "managed-missing-field",
        project: "/tmp/rhino-local-aic-test",
        managedFixtures: [MANAGED_FIXTURE],
      })
    ).rejects.toThrow(/did not contain the declared field "userName"/);
    expect(
      fake.httpCalls.some(
        (call) => call.method === "POST" && call.url.includes("/authenticate")
      )
    ).toBe(false);
  });

  it("refuses a readable alpha_user id before creating the fixture", async () => {
    const fixture: ManagedFixture = {
      type: "managed/alpha_user",
      record: { _id: "readable-user", userName: "alice" },
    };
    const fake = mockTenant();

    await expect(
      runAicLane(managedCase(fixture), SUBJECT, {
        io: fake.io,
        runId: "managed-user-id",
        project: "/tmp/rhino-local-aic-test",
        managedFixtures: [fixture],
      })
    ).rejects.toThrow(
      /managed\/alpha_user.*readable-user.*36-character UUID.*managed\/alpha_role/
    );
    expect(
      fake.httpCalls.some(
        (call) => call.method === "POST" && call.url.includes("?_action=create")
      )
    ).toBe(false);
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
    ).rejects.toThrow(new RegExp(`collision.*${MANAGED_ID}`));
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
    ).rejects.toThrow(new RegExp(`${MANAGED_ID}.*not readable after create`));
    expect(
      fake.httpCalls.some(
        (call) => call.method === "POST" && call.url.includes("/authenticate")
      )
    ).toBe(false);
    expect(
      fake.httpCalls.some(
        (call) =>
          call.method === "DELETE" && call.url.includes(`/${MANAGED_ID}`)
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
        call.method === "DELETE" && call.url.includes(`/${MANAGED_ID}`)
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
        expect.stringMatching(new RegExp(`${MANAGED_ID}.*HTTP 500`))
      );
    } finally {
      warning.mockRestore();
    }
  });

  it("does not number the session-minting journey as a subject pass", async () => {
    const fake = mockTenant();
    await runAicLane(
      caseWith({
        given: { existingSession: { UserId: "alice", tier: "gold" } },
      }),
      SUBJECT,
      {
        io: fake.io,
        runId: "session1",
        project: "/tmp/rhino-local-aic-test",
      }
    );
    const authenticates = fake.httpCalls.filter(
      (call) => call.method === "POST" && call.url.includes("/authenticate")
    );
    expect(authenticates).toHaveLength(2);
    const mint = authenticates.find((call) =>
      call.url.includes("authIndexValue=rl-aic-session1-session")
    );
    const subject = authenticates.find((call) =>
      /authIndexValue=rl-aic-session1(?:&|$)/.test(call.url)
    );
    const mintId = headerValues(mint?.headerLines ?? [], TX_HEADER)[0];
    const subjectId = headerValues(subject?.headerLines ?? [], TX_HEADER)[0];
    expect(mintId !== undefined && mintId.length > 0).toBe(true);
    expect(subjectId).toMatch(/-01$/);
    expect(mintId === subjectId).toBe(false);
    expect(subjectId?.startsWith(`${mintId}-`)).toBe(false);
    expect(peekAicTrace()?.passIds).toEqual([subjectId]);
  });

  it("ignores an author-supplied transaction header rather than sending two", async () => {
    const fake = mockTenant();
    await runAicLane(
      caseWith({
        given: { requestHeaders: { [TX_HEADER]: ["author-supplied"] } },
      }),
      SUBJECT,
      {
        io: fake.io,
        runId: "txhdr",
        project: "/tmp/rhino-local-aic-test",
      }
    );
    const authenticate = fake.httpCalls.find(
      (call) => call.method === "POST" && call.url.includes("/authenticate")
    );
    const sent = headerValues(authenticate?.headerLines ?? [], TX_HEADER);
    expect(sent).toHaveLength(1);
    expect(sent[0]).not.toBe("author-supplied");
    expect(sent[0]).toMatch(/-01$/);
  });
});

function managedCase(fixture: ManagedFixture = MANAGED_FIXTURE) {
  return caseWith({
    given: {
      managed: {
        [fixture.type]: [fixture.record],
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
    managedReadOmit?: readonly string[];
    managedDeleteStatus?: number;
    normalizedScriptSource?: string;
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
  let managedRecord: Record<string, unknown> | undefined;
  const resources = new Map<string, Record<string, unknown>>();

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
        if (path.includes("/serverinfo/")) {
          return json(200, { cookieName: "testCookie" });
        }
        if (path.startsWith("/openidm/managed/")) {
          const status = options.managedReadStatus ?? (managedRecord === undefined ? 404 : 200);
          const readRecord =
            managedRecord === undefined ? undefined : { ...managedRecord };
          for (const key of options.managedReadOmit ?? []) {
            delete readRecord?.[key];
          }
          return json(status, status === 200 ? readRecord : { code: status });
        }
        if (existing.has(path)) {
          return json(200, { _id: "already-there" });
        }
        const resource = resources.get(path);
        if (resource !== undefined) {
          return json(200, resource);
        }
        return json(404, { code: 404 });
      }
      if (req.method === "PUT") {
        const body = JSON.parse(String(req.body)) as Record<string, unknown>;
        resources.set(path, readBack(path, body, options.normalizedScriptSource));
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
          managedRecord = JSON.parse(String(req.body)) as Record<string, unknown>;
        }
        return json(status, status === 201 ? managedRecord : { code: status });
      }
      if (req.method === "POST" && path.endsWith("/authenticate")) {
        const tree = url.searchParams.get("authIndexValue") ?? "";
        if (tree.endsWith("-session")) {
          return json(200, { tokenId: "minted-session-token" });
        }
        return json(options.authenticateStatus ?? 200, authenticateBody);
      }
      return json(500, { message: `unexpected ${req.method} ${path}` });
    },
  };
  return { io, httpCalls, aicArgs };
}

function readBack(
  path: string,
  body: Record<string, unknown>,
  normalizedScriptSource?: string
): Record<string, unknown> {
  if (path.includes("/scripts/")) {
    return {
      ...body,
      ...(normalizedScriptSource === undefined
        ? {}
        : { script: Buffer.from(normalizedScriptSource, "utf8").toString("base64") }),
      createdBy: "server",
      creationDate: 1,
      lastModifiedBy: "server",
      lastModifiedDate: 1,
    };
  }
  if (path.includes("/nodes/ScriptedDecisionNode/")) {
    return {
      ...body,
      _id: path.slice(path.lastIndexOf("/") + 1),
      _rev: "rev",
      _type: { server: true },
      _outcomes: [],
    };
  }
  const nodes = body.nodes as Record<string, Record<string, unknown>>;
  return {
    ...body,
    _id: path.slice(path.lastIndexOf("/") + 1),
    _rev: "rev",
    innerTreeOnly: false,
    noSession: false,
    mustRun: false,
    transactionalOnly: false,
    nodes: Object.fromEntries(
      Object.entries(nodes).map(([id, node]) => [id, { version: "1.0", ...node }])
    ),
  };
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
