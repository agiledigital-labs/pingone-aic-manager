import { describe, expect, it } from "vitest";
import { encodeQueryFilter, tenantIdmHandle } from "../../src/aic/idm.ts";
import type { HttpRequest, HttpResponse } from "../../src/aic/http.ts";
import type { AicIo, TenantSession } from "../../src/aic/tenant.ts";

const SESSION: TenantSession = {
  tenantName: "test",
  baseUrl: "https://tenant.example.com",
  token: "test-token",
  project: "/tmp/rhino-local-aic-test",
};

describe("tenantIdmHandle", () => {
  it("returns the tenant's full materialized record without projecting it", async () => {
    const record = {
      _id: "alice",
      _rev: "3",
      userName: "alice",
      description: null,
      roles: [],
    };
    const fake = new FakeIdmIo([json(200, record)]);

    await expect(
      tenantIdmHandle(fake.io, SESSION).read("managed/alpha_user/alice")
    ).resolves.toEqual(record);
    expect(fake.calls[0]).toMatchObject({
      method: "GET",
      url: "https://tenant.example.com/openidm/managed/alpha_user/alice",
    });
  });

  it("maps a read miss to null", async () => {
    const fake = new FakeIdmIo([json(404, { code: 404, reason: "Not Found" })]);
    await expect(
      tenantIdmHandle(fake.io, SESSION).read("managed/alpha_user/missing")
    ).resolves.toBeNull();
  });

  it("returns query rows and sends the actual CREST filter", async () => {
    const rows = [{ _id: "alice", description: null, roles: [] }];
    const fake = new FakeIdmIo([
      json(200, { result: rows, resultCount: 1, totalPagedResults: -1 }),
    ]);

    await expect(
      tenantIdmHandle(fake.io, SESSION).query("managed/alpha_user", {
        userName: "alice",
        active: true,
        score: 3.5,
      })
    ).resolves.toEqual(rows);
    expect(new URL(fake.calls[0]?.url ?? "").searchParams.get("_queryFilter")).toBe(
      'userName eq "alice" and active eq true and score eq 3.5'
    );
  });

  it("does not pretend it can detect an unknown field", async () => {
    const fake = new FakeIdmIo([
      json(200, { result: [], resultCount: 0, totalPagedResults: -1 }),
    ]);
    await expect(
      tenantIdmHandle(fake.io, SESSION).query("managed/alpha_user", {
        nosuchfield: "value",
      })
    ).resolves.toEqual([]);
    expect(fake.calls).toHaveLength(1);
  });

  it("accepts the deleted record on 200 and treats 404 as already absent", async () => {
    const fake = new FakeIdmIo([
      json(200, { _id: "alice", userName: "alice", description: null }),
      json(404, { code: 404 }),
    ]);
    const idm = tenantIdmHandle(fake.io, SESSION);

    await expect(idm.delete("managed/alpha_user/alice")).resolves.toBeUndefined();
    await expect(idm.delete("managed/alpha_user/alice")).resolves.toBeUndefined();
    expect(fake.calls.map((call) => call.method)).toEqual(["DELETE", "DELETE"]);
  });

  it.each([
    ["HTTP 204", { status: 204, headers: [], body: "" }],
    ["an empty HTTP 200 object", json(200, {})],
  ])("rejects %s instead of assuming status alone proves deletion", async (_label, response) => {
    const fake = new FakeIdmIo([response]);
    await expect(
      tenantIdmHandle(fake.io, SESSION).delete("managed/alpha_user/alice")
    ).rejects.toThrow(/expected 200 or 404|no deleted record/);
  });
});

describe("encodeQueryFilter", () => {
  it("uses true for an empty object and preserves nested CREST field paths", () => {
    expect(encodeQueryFilter({})).toBe("true");
    expect(encodeQueryFilter({ "/name/last": "Smith" })).toBe(
      '/name/last eq "Smith"'
    );
  });

  it.each([
    ["a quoted string", { sn: 'Twelve" B' }],
    ["null", { mail: null }],
    ["an array", { roles: ["reviewer"] }],
    ["an object", { name: { first: "Alice" } }],
    ["unsafe field syntax", { "userName or true": "alice" }],
  ])("refuses %s rather than emitting a silently wrong filter", (_label, filter) => {
    expect(() => encodeQueryFilter(filter)).toThrow(/cannot be encoded|unencodable/);
  });
});

class FakeIdmIo {
  readonly calls: HttpRequest[] = [];
  readonly #responses: HttpResponse[];

  constructor(responses: HttpResponse[]) {
    this.#responses = responses;
  }

  readonly io: AicIo = {
    aic: () => Promise.reject(new Error("unexpected CLI call")),
    http: (request) => {
      this.calls.push(request);
      const response = this.#responses.shift();
      if (response === undefined) {
        return Promise.reject(new Error("unexpected HTTP call"));
      }
      return Promise.resolve(response);
    },
  };
}

function json(status: number, body: unknown): HttpResponse {
  return { status, headers: [], body: JSON.stringify(body) };
}
