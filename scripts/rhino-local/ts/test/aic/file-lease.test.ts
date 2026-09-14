import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { HARNESS_CALLBACK_ID } from "../../src/aic/constants.ts";
import { AicFileLease } from "../../src/aic/file-lease.ts";
import { createLeaseIdentity } from "../../src/aic/lease-identity.ts";
import {
  leaseStatePaths,
  newLeaseJournal,
  readLeaseJournal,
  writeLeaseJournal,
} from "../../src/aic/lease-lock.ts";
import type { HttpRequest, HttpResponse } from "../../src/aic/http.ts";
import type { AicIo } from "../../src/aic/tenant.ts";
import { headerValues } from "../../src/aic/http.ts";
import { TX_HEADER } from "../../src/aic/txid.ts";
import type { ManagedFixture } from "../../src/aic/managed.ts";
import { runAicChain } from "../../src/aic/run.ts";
import type { Given } from "../../src/case/types.ts";
import { makeEffects } from "../case/helpers.ts";
import { caseWith } from "./helpers.ts";

const SOURCE = 'action.goTo("done");\n';
const MANAGED_FIXTURE: ManagedFixture = {
  type: "managed/alpha_user",
  record: {
    _id: "00000000-0000-4000-8000-000000000099",
    userName: "leased-user",
  },
};

describe("AicFileLease", () => {
  it("returns the same effects as the one-shot compatibility facade", async () => {
    const directFake = new FakeLeaseTenant();
    const input = request("parity");
    const direct = await runAicChain(input.cases, SOURCE, {
      io: directFake.io,
      project: "/tmp/rhino-local-aic-test",
      runId: "effect-parity",
      replies: input.replies,
    });

    const leasedFake = new FakeLeaseTenant();
    const lease = makeLease(leasedFake);
    await lease.open();
    const report = await lease.run(input);
    const leased = report.passes.map((pass) => pass.aic.effects);
    expect(leased).toEqual(direct);
    await lease.close();
  });

  it("applies the same managed-provenance refusal in both runners", async () => {
    const input = request("wrong provenance", "alpha", {
      managed: { [MANAGED_FIXTURE.type]: [MANAGED_FIXTURE.record] },
    });
    const directFake = new FakeLeaseTenant();
    await expect(
      runAicChain(input.cases, SOURCE, {
        io: directFake.io,
        project: "/tmp/rhino-local-aic-test",
        runId: "provenance",
        replies: input.replies,
        managedFixtures: [],
      })
    ).rejects.toThrow(/managed fixture provenance/);
    expect(directFake.calls).toEqual([]);

    const leasedFake = new FakeLeaseTenant();
    const lease = makeLease(leasedFake);
    await lease.open();
    const arms = leasedFake.events.filter((event) => event === "arm").length;
    await expect(
      lease.run({ ...input, managedFixtures: [] })
    ).rejects.toThrow(/managed fixture provenance/);
    expect(leasedFake.events.filter((event) => event === "arm")).toHaveLength(
      arms
    );
    await lease.close();
  });

  it("provisions once, arms and invokes twice, then tears down once", async () => {
    const fake = new FakeLeaseTenant();
    const lease = makeLease(fake);
    await lease.open();
    const treeName = lease.identity.treeName;
    await lease.run(request("first"));
    await lease.run(request("second"));
    await lease.close();
    await lease.close();

    expect(fake.calls.length + fake.aicCalls).toBe(44);
    expect(fake.calls.filter((call) => call.method === "POST")).toHaveLength(2);
    expect(fake.calls.filter((call) => call.method === "DELETE")).toHaveLength(9);
    expect(fake.putsFor(`/trees/${treeName}`)).toHaveLength(1);
    expect(fake.putsFor(`/scripts/${lease.identity.ids.subjectScript}`)).toHaveLength(3);
    expect(treeName).toMatch(/^rl-aic-[0-9a-f]{20}$/);
  });

  it("serializes concurrent cases around the shared subject slot", async () => {
    const fake = new FakeLeaseTenant({ delayFirstArm: true });
    const lease = makeLease(fake);
    await lease.open();
    await Promise.all([lease.run(request("first")), lease.run(request("second"))]);
    const relevant = fake.events.filter((event) =>
      event === "arm" || event === "confirm-arm" || event === "authenticate"
    );
    expect(relevant).toEqual([
      "arm",
      "confirm-arm",
      "authenticate",
      "arm",
      "confirm-arm",
      "authenticate",
    ]);
    await lease.close();
  });

  it("refuses a realm mismatch before mutating the subject", async () => {
    const fake = new FakeLeaseTenant();
    const lease = makeLease(fake);
    await lease.open();
    const before = fake.putsFor(`/scripts/${lease.identity.ids.subjectScript}`).length;
    await expect(lease.run(request("wrong", "bravo"))).rejects.toThrow(/differs from leased realm/);
    expect(fake.putsFor(`/scripts/${lease.identity.ids.subjectScript}`)).toHaveLength(before);
    await lease.close();
  });

  it("fails unsupported input by default before mutating the subject", async () => {
    const fake = new FakeLeaseTenant();
    const lease = makeLease(fake);
    await lease.open();
    const arms = fake.events.filter((event) => event === "arm").length;
    await expect(
      lease.run(request("legacy", "alpha", { engine: "legacy" }))
    ).rejects.toThrow(/AIC lane unsupported/);
    expect(fake.events.filter((event) => event === "arm")).toHaveLength(arms);
    await lease.close();
  });

  it("only skips unsupported input when explicitly configured", async () => {
    const fake = new FakeLeaseTenant();
    const lease = new AicFileLease({
      id: "file-lease-test",
      suiteName: "file lease test",
      source: SOURCE,
      outcomes: ["done"],
      unsupported: "skip",
      project: "/tmp/rhino-local-aic-test",
      io: fake.io,
    });
    await lease.open();
    const arms = fake.events.filter((event) => event === "arm").length;
    const report = await lease.run(
      request("legacy", "alpha", { engine: "legacy" })
    );
    expect(report.passes[0]?.aic.skipped).toMatch(/legacy engine/);
    expect(fake.events.filter((event) => event === "arm")).toHaveLength(arms);
    await lease.close();
  });

  it("treats create status on subject replacement as staleness", async () => {
    const fake = new FakeLeaseTenant({ armStatus: 201 });
    const lease = makeLease(fake);
    await lease.open();
    await expect(lease.run(request("stale"))).rejects.toThrow(/expected 200/);
    expect(fake.events).not.toContain("authenticate");
    await lease.close();
  });

  it.each([
    ["invocation nonce", { wrongNonce: true }],
    ["result manifest", { wrongLeaseDigest: true }],
  ])("rejects a mismatched runtime %s", async (_label, options) => {
    const fake = new FakeLeaseTenant(options);
    const lease = makeLease(fake);
    await lease.open();
    await expect(lease.run(request("mismatch"))).rejects.toThrow(/runtime lease manifest/);
    await lease.close();
  });

  it("cleans only resources created before a partial-open confirmation failure", async () => {
    const fake = new FakeLeaseTenant({ corruptCreateConfirmation: 2 });
    const lease = makeLease(fake);
    await expect(lease.open()).rejects.toThrow(/confirming read/);
    expect(fake.calls.filter((call) => call.method === "DELETE")).toHaveLength(2);
    await lease.close();
  });

  it("probes an old journal's explicit outcome ids after the vocabulary shrinks", async () => {
    const stateDir = await mkdtemp(join(tmpdir(), "rhino-local-old-journal-"));
    try {
      const old = createLeaseIdentity({
        id: "file-lease-test",
        source: SOURCE,
        outcomes: ["done", "old-outcome"],
        ownerToken: "old-owner",
      });
      const paths = leaseStatePaths("https://tenant.example.com", old, stateDir);
      const oldResult = old.ids.resultScripts["old-outcome"] as string;
      await writeLeaseJournal(
        paths.journalPath,
        newLeaseJournal(paths, old, "alpha", [{ kind: "script", id: oldResult }])
      );
      const fake = new FakeLeaseTenant();
      fake.seed(`/am/json/realms/root/realms/alpha/scripts/${oldResult}`, {
        _id: oldResult,
      });
      const lease = new AicFileLease({
        id: "file-lease-test",
        suiteName: "file lease test",
        source: SOURCE,
        outcomes: ["done"],
        project: "/tmp/rhino-local-aic-test",
        stateDir,
        io: fake.io,
      });
      await expect(lease.open()).rejects.toThrow(new RegExp(oldResult));
      expect(
        fake.calls.some((call) => new URL(call.url).pathname.endsWith(oldResult))
      ).toBe(true);
    } finally {
      await rm(stateDir, { recursive: true, force: true });
    }
  });

  it("retains the journal when teardown only partially deletes the graph", async () => {
    const stateDir = await mkdtemp(join(tmpdir(), "rhino-local-partial-close-"));
    try {
      const fake = new FakeLeaseTenant({ deleteFailureAt: 2 });
      const lease = new AicFileLease({
        id: "partial-close",
        suiteName: "partial close",
        source: SOURCE,
        outcomes: ["done"],
        project: "/tmp/rhino-local-aic-test",
        stateDir,
        io: fake.io,
      });
      await lease.open();
      const paths = leaseStatePaths(
        "https://tenant.example.com",
        lease.identity,
        stateDir
      );
      await expect(lease.close()).rejects.toThrow(/cleanup failed/);
      expect(await readLeaseJournal(paths.journalPath)).toMatchObject({
        aicId: "partial-close",
      });
      expect(fake.calls.filter((call) => call.method === "DELETE")).toHaveLength(9);
    } finally {
      await rm(stateDir, { recursive: true, force: true });
    }
  });

  it("brackets a managed run with create/read/delete and clears its journal entry", async () => {
    const stateDir = await mkdtemp(join(tmpdir(), "rhino-local-managed-lease-"));
    try {
      const fake = new FakeLeaseTenant();
      const lease = new AicFileLease({
        id: "managed-lease",
        suiteName: "managed lease",
        source: SOURCE,
        outcomes: ["done"],
        project: "/tmp/rhino-local-aic-test",
        stateDir,
        io: fake.io,
      });
      await lease.open();
      await lease.run(
        request("managed", "alpha", {
          managed: { [MANAGED_FIXTURE.type]: [MANAGED_FIXTURE.record] },
        }, [MANAGED_FIXTURE])
      );
      const create = fake.events.indexOf("managed-create");
      const read = fake.events.indexOf("managed-read");
      const authenticate = fake.events.indexOf("authenticate");
      const remove = fake.events.indexOf("managed-delete");
      expect(create).toBeGreaterThanOrEqual(0);
      expect(read).toBeGreaterThan(create);
      expect(authenticate).toBeGreaterThan(read);
      expect(remove).toBeGreaterThan(authenticate);
      const paths = leaseStatePaths(
        "https://tenant.example.com",
        lease.identity,
        stateDir
      );
      expect((await readLeaseJournal(paths.journalPath))?.managedFixtures).toEqual([]);
      await lease.close();
    } finally {
      await rm(stateDir, { recursive: true, force: true });
    }
  });

  it("deletes managed fixtures when subject authentication fails", async () => {
    const fake = new FakeLeaseTenant({ subjectAuthStatus: 401 });
    const lease = makeLease(fake);
    await lease.open();
    await expect(
      lease.run(
        request("managed-failure", "alpha", {
          managed: { [MANAGED_FIXTURE.type]: [MANAGED_FIXTURE.record] },
        }, [MANAGED_FIXTURE])
      )
    ).rejects.toThrow(/authenticate HTTP 401/);
    expect(fake.events).toContain("managed-delete");
    await lease.close();
  });

  it("deletes managed fixtures when subject replacement reports create", async () => {
    const fake = new FakeLeaseTenant({ armStatus: 201 });
    const lease = makeLease(fake);
    await lease.open();
    await expect(
      lease.run(
        request("managed-update-failure", "alpha", {
          managed: { [MANAGED_FIXTURE.type]: [MANAGED_FIXTURE.record] },
        }, [MANAGED_FIXTURE])
      )
    ).rejects.toThrow(/expected 200/);
    expect(fake.events).toContain("managed-delete");
    await lease.close();
  });

  it("fails closed on a managed create collision before subject mutation", async () => {
    const fake = new FakeLeaseTenant({ managedCreateStatus: 412 });
    const lease = makeLease(fake);
    await lease.open();
    const arms = fake.events.filter((event) => event === "arm").length;
    await expect(
      lease.run(
        request("managed-collision", "alpha", {
          managed: { [MANAGED_FIXTURE.type]: [MANAGED_FIXTURE.record] },
        }, [MANAGED_FIXTURE])
      )
    ).rejects.toThrow(/managed fixture collision/);
    expect(fake.events.filter((event) => event === "arm")).toHaveLength(arms);
    await lease.close();
  });

  it("reuses one lazy session minter and one cookie-name lookup", async () => {
    const fake = new FakeLeaseTenant();
    const lease = makeLease(fake);
    await lease.open();
    await lease.run(request("session-a", "alpha", { existingSession: { tier: "a" } }));
    await lease.run(request("session-b", "alpha", { existingSession: { tier: "b" } }));
    const minterPuts = fake.putsFor(`/scripts/${lease.identity.ids.sessionScript}`);
    expect(minterPuts).toHaveLength(2);
    const minterSources = minterPuts.map((call) => {
      const body = JSON.parse(String(call.body)) as { script: string };
      return Buffer.from(body.script, "base64").toString("utf8");
    });
    expect(minterSources[0]).toContain('"tier", "a"');
    expect(minterSources[1]).toContain('"tier", "b"');
    expect(fake.calls.filter((call) => call.url.includes("/serverinfo/"))).toHaveLength(1);
    expect(fake.subjectTransactionIds).toHaveLength(2);
    expect(fake.subjectTransactionIds.every((id) => /-01$/.test(id))).toBe(true);
    await lease.close();
  });
});

function makeLease(fake: FakeLeaseTenant): AicFileLease {
  return new AicFileLease({
    id: "file-lease-test",
    suiteName: "file lease test",
    source: SOURCE,
    outcomes: ["done"],
    project: "/tmp/rhino-local-aic-test",
    io: fake.io,
  });
}

function request(
  name: string,
  realm = "alpha",
  given: Given = {},
  managedFixtures?: readonly ManagedFixture[]
) {
  const kase = caseWith({
    name,
    script: SOURCE,
    outcomes: ["done"],
    given: { ...given, realm },
    expect: { outcome: "done" },
  });
  return {
    cases: [kase],
    source: SOURCE,
    localEffects: [
      makeEffects({
        outcome: "done",
        evidence: {
          stateBuckets: "unified",
          ambientState: {},
          unbucketedState: [],
          unobservedChannels: ["openidm", "http", "logs"],
        },
      }),
    ],
    replies: [],
    ...(managedFixtures === undefined ? {} : { managedFixtures }),
  };
}

interface FakeLeaseOptions {
  armStatus?: number;
  delayFirstArm?: boolean;
  wrongNonce?: boolean;
  wrongLeaseDigest?: boolean;
  corruptCreateConfirmation?: number;
  deleteFailureAt?: number;
  subjectAuthStatus?: number;
  managedCreateStatus?: number;
}

class FakeLeaseTenant {
  readonly calls: HttpRequest[] = [];
  readonly events: string[] = [];
  aicCalls = 0;
  readonly subjectTransactionIds: string[] = [];
  readonly #resources = new Map<string, Record<string, unknown>>();
  readonly #options: FakeLeaseOptions;
  #staticCreates = 0;
  #armCount = 0;
  #nonce = "";
  #subjectDigest = "";
  #leaseDigest = "";
  #deleteCount = 0;
  #sessionTree = "";
  #managedRecord: Record<string, unknown> | undefined;

  constructor(options: FakeLeaseOptions = {}) {
    this.#options = options;
  }

  readonly io: AicIo = {
    aic: (args) => {
      this.aicCalls += 1;
      if (args.includes("ctx") && args.includes("list")) {
        return Promise.resolve({
          status: 0,
          stdout: JSON.stringify([
            { current: true, name: "sandbox", base_url: "https://tenant.example.com" },
          ]),
          stderr: "",
        });
      }
      return Promise.resolve({ status: 0, stdout: "test-token\n", stderr: "" });
    },
    http: async (req) => {
      this.calls.push(req);
      const path = new URL(req.url).pathname;
      const url = new URL(req.url);
      if (req.method === "GET") {
        if (path.includes("/serverinfo/")) {
          return json(200, { cookieName: "testCookie" });
        }
        if (path.startsWith("/openidm/managed/")) {
          this.events.push("managed-read");
          return this.#managedRecord === undefined
            ? json(404, { code: 404 })
            : json(200, this.#managedRecord);
        }
        if (path.includes("/scripts/") && this.#armCount > 0 && this.#resources.has(path)) {
          this.events.push("confirm-arm");
        }
        return this.#resources.has(path)
          ? json(200, this.#resources.get(path))
          : json(404, { code: 404 });
      }
      if (req.method === "PUT") {
        const body = JSON.parse(String(req.body)) as Record<string, unknown>;
        const exists = this.#resources.has(path);
        if (exists && path.includes("/scripts/") && String(body.name).endsWith("-subject")) {
          this.#armCount += 1;
          this.events.push("arm");
          if (this.#options.delayFirstArm === true && this.#armCount === 1) {
            await new Promise((resolve) => setTimeout(resolve, 10));
          }
          const source = Buffer.from(String(body.script), "base64").toString("utf8");
          this.#nonce = capture(source, /invocationNonce: "([^"]+)"/);
          this.#subjectDigest = capture(source, /subjectDigest: "([0-9a-f]+)"/);
        } else {
          this.#staticCreates += 1;
          if (path.includes("/scripts/") && String(body.name).includes("-result-")) {
            const source = Buffer.from(String(body.script), "base64").toString("utf8");
            const digest = /leaseDigest: "([0-9a-f]+)"/.exec(source)?.[1];
            if (digest !== undefined) {
              this.#leaseDigest = digest;
            }
          }
          if (path.includes("/trees/") && Object.keys(body.nodes as object).length === 1) {
            this.#sessionTree = decodeURIComponent(path.slice(path.lastIndexOf("/") + 1));
          }
        }
        const read = readBack(path, body);
        if (this.#options.corruptCreateConfirmation === this.#staticCreates) {
          read.enabled = false;
        }
        this.#resources.set(path, read);
        return json(exists ? (this.#options.armStatus ?? 200) : 201, {});
      }
      if (req.method === "DELETE") {
        if (path.startsWith("/openidm/managed/")) {
          this.events.push("managed-delete");
          this.#managedRecord = undefined;
          return json(200, {});
        }
        this.#deleteCount += 1;
        if (this.#deleteCount === this.#options.deleteFailureAt) {
          return json(500, { code: 500 });
        }
        this.#resources.delete(path);
        return json(200, {});
      }
      if (
        req.method === "POST" &&
        path.startsWith("/openidm/managed/") &&
        url.searchParams.get("_action") === "create"
      ) {
        this.events.push("managed-create");
        const status = this.#options.managedCreateStatus ?? 201;
        if (status === 201) {
          this.#managedRecord = JSON.parse(String(req.body)) as Record<string, unknown>;
        }
        return json(status, this.#managedRecord ?? { code: status });
      }
      if (url.searchParams.get("authIndexValue") === this.#sessionTree) {
        this.events.push("session-authenticate");
        return json(200, { tokenId: `session-${this.events.length}` });
      }
      this.events.push("authenticate");
      const transactionId = headerValues(req.headerLines, TX_HEADER)[0];
      if (transactionId !== undefined) {
        this.subjectTransactionIds.push(transactionId);
      }
      const status = this.#options.subjectAuthStatus ?? 200;
      return json(status, status === 200 ? {
        callbacks: [
          {
            type: "HiddenValueCallback",
            output: [
              { name: "id", value: HARNESS_CALLBACK_ID },
              {
                name: "value",
                value: JSON.stringify({
                  outcome: "done",
                  before: {},
                  final: {},
                  invocationNonce: this.#options.wrongNonce === true ? "wrong" : this.#nonce,
                  subjectDigest: this.#subjectDigest,
                  leaseDigest:
                    this.#options.wrongLeaseDigest === true ? "wrong" : this.#leaseDigest,
                }),
              },
            ],
          },
        ],
      } : { code: status });
    },
  };

  putsFor(suffix: string): HttpRequest[] {
    return this.calls.filter(
      (call) => call.method === "PUT" && new URL(call.url).pathname.endsWith(suffix)
    );
  }

  seed(path: string, body: Record<string, unknown>): void {
    this.#resources.set(path, body);
  }
}

function readBack(path: string, body: Record<string, unknown>): Record<string, unknown> {
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
    return { ...body, _id: "node", _rev: "rev", _type: {}, _outcomes: [] };
  }
  const nodes = body.nodes as Record<string, Record<string, unknown>>;
  return {
    ...body,
    _id: "tree",
    _rev: "rev",
    innerTreeOnly: false,
    noSession: false,
    mustRun: false,
    transactionalOnly: false,
    nodes: Object.fromEntries(
      Object.entries(nodes).map(([id, node]) => [id, { ...node, version: "1.0" }])
    ),
  };
}

function capture(source: string, pattern: RegExp): string {
  return pattern.exec(source)?.[1] ?? "";
}

function json(status: number, body: unknown): HttpResponse {
  return { status, headers: [], body: JSON.stringify(body) };
}
