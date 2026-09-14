import { describe, expect, it } from "vitest";
import { HARNESS_CALLBACK_ID } from "../../src/aic/constants.ts";
import { AicFileLease } from "../../src/aic/file-lease.ts";
import type { HttpRequest, HttpResponse } from "../../src/aic/http.ts";
import type { AicIo } from "../../src/aic/tenant.ts";
import { makeEffects } from "../case/helpers.ts";
import { caseWith } from "./helpers.ts";

const SOURCE = 'action.goTo("done");\n';

describe("AicFileLease", () => {
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

function request(name: string, realm = "alpha") {
  const kase = caseWith({
    name,
    script: SOURCE,
    outcomes: ["done"],
    given: { realm },
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
  };
}

interface FakeLeaseOptions {
  armStatus?: number;
  delayFirstArm?: boolean;
  wrongNonce?: boolean;
  wrongLeaseDigest?: boolean;
  corruptCreateConfirmation?: number;
}

class FakeLeaseTenant {
  readonly calls: HttpRequest[] = [];
  readonly events: string[] = [];
  aicCalls = 0;
  readonly #resources = new Map<string, Record<string, unknown>>();
  readonly #options: FakeLeaseOptions;
  #staticCreates = 0;
  #armCount = 0;
  #nonce = "";
  #subjectDigest = "";
  #leaseDigest = "";

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
      if (req.method === "GET") {
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
        }
        const read = readBack(path, body);
        if (this.#options.corruptCreateConfirmation === this.#staticCreates) {
          read.enabled = false;
        }
        this.#resources.set(path, read);
        return json(exists ? (this.#options.armStatus ?? 200) : 201, {});
      }
      if (req.method === "DELETE") {
        this.#resources.delete(path);
        return json(200, {});
      }
      this.events.push("authenticate");
      return json(200, {
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
      });
    },
  };

  putsFor(suffix: string): HttpRequest[] {
    return this.calls.filter(
      (call) => call.method === "PUT" && new URL(call.url).pathname.endsWith(suffix)
    );
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
