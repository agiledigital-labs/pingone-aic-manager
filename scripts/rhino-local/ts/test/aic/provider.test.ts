import { afterEach, describe, expect, it } from "vitest";
import { generateKeyPairSync, verify } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { HttpRequest, HttpResponse } from "../../src/aic/http.ts";
import { aicCliProvider, type AicIo, connectTenant } from "../../src/aic/tenant.ts";
import {
  logKeysReader,
  providerFromEnv,
  serviceAccountProvider,
  setTenantProvider,
  TENANT_ENV,
  TenantProviderError,
  tokenCallbackProvider,
  type HttpSend,
  type TenantProvider,
} from "../../src/aic/provider.ts";

const baseUrl = "https://tenant.example.com";
const id = "service-account-id";
const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const jwk = privateKey.export({ format: "jwk" });

afterEach(() => setTenantProvider(undefined));

describe("serviceAccountProvider", () => {
  it("posts a signed JWT bearer assertion and caches only initial mints", async () => {
    let now = 1_800_000_000;
    const requests: HttpRequest[] = [];
    const send: HttpSend = async (request) => {
      requests.push(request);
      return response(
        200,
        JSON.stringify({ access_token: `token-${requests.length}`, expires_in: 900 })
      );
    };
    const provider = serviceAccountProvider({
      baseUrl,
      serviceAccountId: id,
      jwk,
      http: send,
      now: () => now,
    });
    expect(await provider.getToken({ reason: "initial" })).toBe("token-1");
    expect(await provider.getToken({ reason: "initial" })).toBe("token-1");
    expect(requests).toHaveLength(1);
    const req = requests[0]!;
    expect(req.url).toBe(`${baseUrl}/am/oauth2/access_token`);
    expect(req.method).toBe("POST");
    const fields = new URLSearchParams(req.body);
    expect(fields.get("client_id")).toBe("service-account");
    expect(fields.get("grant_type")).toBe("urn:ietf:params:oauth:grant-type:jwt-bearer");
    expect(fields.get("scope")).toBe("fr:am:* fr:idm:* fr:idc:esv:*");
    const assertion = fields.get("assertion")!;
    const [header, payload, signature] = assertion.split(".");
    expect(JSON.parse(Buffer.from(header!, "base64url").toString())).toMatchObject({
      alg: "RS256",
    });
    const claims = JSON.parse(Buffer.from(payload!, "base64url").toString()) as Record<
      string,
      unknown
    >;
    expect(claims).toMatchObject({
      iss: id,
      sub: id,
      aud: `${baseUrl}/am/oauth2/access_token`,
      exp: now + 180,
    });
    expect(
      verify(
        "RSA-SHA256",
        Buffer.from(`${header}.${payload}`),
        publicKey,
        Buffer.from(signature!, "base64url")
      )
    ).toBe(true);
    await provider.getToken({ reason: "rejected", rejected: "token-1" });
    expect(requests).toHaveLength(2);
    const second = new URLSearchParams(requests[1]!.body).get("assertion")!;
    expect(JSON.parse(Buffer.from(second.split(".")[1]!, "base64url").toString()).jti).not.toBe(
      claims.jti
    );
    now += 841;
    await provider.getToken({ reason: "initial" });
    expect(requests).toHaveLength(3);
  });

  it("rejects bad status, public keys, non-RSA keys, and malformed JSON", async () => {
    const failed = serviceAccountProvider({
      baseUrl,
      serviceAccountId: id,
      jwk,
      http: async () => response(401, "no"),
    });
    await expect(failed.getToken({ reason: "initial" })).rejects.toMatchObject({
      name: "TenantProviderError",
      message: expect.stringContaining("401"),
    });
    expect(() =>
      serviceAccountProvider({
        baseUrl,
        serviceAccountId: id,
        jwk: publicKey.export({ format: "jwk" }),
      })
    ).toThrow(TenantProviderError);
    expect(() =>
      serviceAccountProvider({ baseUrl, serviceAccountId: id, jwk: { kty: "EC" } as never })
    ).toThrow(TenantProviderError);
    expect(() =>
      serviceAccountProvider({ baseUrl, serviceAccountId: id, jwk: "not-json" })
    ).toThrow(TenantProviderError);
  });
});

describe("callback and URL handling", () => {
  it("forwards initial and rejected requests and rejects blank tokens", async () => {
    const calls: unknown[] = [];
    const provider = tokenCallbackProvider({
      baseUrl,
      getToken: (request) => {
        calls.push(request);
        return " token ";
      },
    });
    await provider.getToken({ reason: "initial" });
    await provider.getToken({ reason: "rejected", rejected: "old" });
    expect(calls).toEqual([{ reason: "initial" }, { reason: "rejected", rejected: "old" }]);
    const blank = tokenCallbackProvider({ baseUrl, getToken: () => "  \n" });
    await expect(blank.getToken({ reason: "initial" })).rejects.toThrow(/empty token/);
  });

  it("normalizes trailing slashes, defaults the name, and refuses paths", async () => {
    expect(
      await tokenCallbackProvider({ baseUrl: `${baseUrl}/`, getToken: () => "x" }).describe()
    ).toEqual({ name: "tenant.example.com", baseUrl });
    expect(() =>
      tokenCallbackProvider({ baseUrl: `${baseUrl}/path`, getToken: () => "x" })
    ).toThrow(/no path/);
  });
});

describe("providerFromEnv", () => {
  const env = (extra: Record<string, string> = {}) => ({
    [TENANT_ENV.url]: baseUrl,
    [TENANT_ENV.serviceAccountId]: id,
    [TENANT_ENV.jwk]: JSON.stringify(jwk),
    ...extra,
  });
  it("returns undefined without URL and validates required configuration", () => {
    expect(providerFromEnv({})).toBeUndefined();
    expect(() => providerFromEnv({ [TENANT_ENV.url]: baseUrl })).toThrow(/SA_ID/);
    expect(() => providerFromEnv(env({ [TENANT_ENV.jwkFile]: "/tmp/key" }))).toThrow(/exactly one/);
    expect(() =>
      providerFromEnv({ [TENANT_ENV.url]: baseUrl, [TENANT_ENV.serviceAccountId]: id })
    ).toThrow(/exactly one/);
    expect(() => providerFromEnv(env({ [TENANT_ENV.logKeyId]: "key" }))).toThrow(/both/);
  });

  it("reads the JWK file variable", () => {
    const dir = mkdtempSync(join(tmpdir(), "rhino-provider-"));
    const file = join(dir, "sa.json");
    writeFileSync(file, JSON.stringify(jwk));
    try {
      expect(
        providerFromEnv({
          [TENANT_ENV.url]: baseUrl,
          [TENANT_ENV.serviceAccountId]: id,
          [TENANT_ENV.jwkFile]: file,
        })
      ).toBeDefined();
      expect(readFileSync(file, "utf8")).toContain('"kty":"RSA"');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("logKeysReader", () => {
  it("sends keys, follows cookies, polls for outcome, and returns partial events on timeout", async () => {
    let clock = 0;
    const requests: HttpRequest[] = [];
    const requestTimes: number[] = [];
    const events: unknown[] = [];
    const reader = logKeysReader(
      baseUrl,
      { id: "key-id", secret: "key-secret" },
      async (req) => {
        requests.push(req);
        requestTimes.push(clock);
        const url = new URL(req.url);
        if (requests.length === 1)
          return response(200, JSON.stringify({ result: [{ n: 1 }], pagedResultsCookie: "next" }));
        if (requests.length === 2) {
          expect(url.searchParams.get("_pagedResultsCookie")).toBe("next");
          return response(200, JSON.stringify({ result: [{ n: 2 }] }));
        }
        events.push({ n: 1 }, { n: 2 }, { payload: { eventName: "AM-ACCESS-OUTCOME" } });
        return response(200, JSON.stringify({ result: events }));
      },
      {
        now: () => clock,
        sleep: async (ms) => {
          clock += ms;
        },
      }
    );
    expect(await reader.transaction("tx", { waitMs: 5000 })).toEqual([
      { n: 1 },
      { n: 2 },
      { payload: { eventName: "AM-ACCESS-OUTCOME" } },
    ]);
    expect(
      requests.every(
        (req) =>
          req.headerLines.some(([name, value]) => name === "x-api-key" && value === "key-id") &&
          req.headerLines.some(([name, value]) => name === "x-api-secret" && value === "key-secret")
      )
    ).toBe(true);
    expect(requests[1]!.url).toContain("_pagedResultsCookie=next");
    expect(clock).toBeGreaterThanOrEqual(2100);
    expect(requestTimes.slice(1).every((time, index) => time - requestTimes[index]! >= 1050)).toBe(
      true
    );

    let timeoutClock = 0;
    let count = 0;
    const partial = logKeysReader(
      baseUrl,
      { id: "i", secret: "s" },
      async () => {
        count++;
        return response(200, JSON.stringify({ result: [{ n: count }] }));
      },
      {
        now: () => timeoutClock,
        sleep: async (ms) => {
          timeoutClock += ms;
        },
      }
    );
    expect(await partial.transaction("tx", { waitMs: 1100 })).toEqual([{ n: 3 }]);
  });

  it("honours Retry-After, spaces requests, and rejects other failures", async () => {
    let clock = 0;
    const sleeps: number[] = [];
    let call = 0;
    const reader = logKeysReader(
      baseUrl,
      { id: "i", secret: "s" },
      async () => {
        call++;
        return call === 1
          ? { ...response(429, "wait"), headers: [["Retry-After", "7"]] }
          : response(
              200,
              JSON.stringify({ result: [{ payload: { eventName: "AM-ACCESS-OUTCOME" } }] })
            );
      },
      {
        now: () => clock,
        sleep: async (ms) => {
          sleeps.push(ms);
          clock += ms;
        },
      }
    );
    await reader.transaction("tx");
    expect(sleeps).toContain(7000);
    expect(sleeps.some((ms) => ms >= 1050)).toBe(true);
    const bad = logKeysReader(baseUrl, { id: "i", secret: "s" }, async () =>
      response(500, "broken")
    );
    await expect(bad.transaction("tx")).rejects.toThrow(/500/);
  });
});

describe("tenant connections and CLI provider", () => {
  const custom = (name = "sandbox"): TenantProvider => ({
    describe: async () => ({ name, baseUrl }),
    getToken: async () => "bearer",
  });
  it("enforces tenant pinning and uses or bypasses the registered provider", async () => {
    await expect(connectTenant(emptyIo(), { tenant: "other", provider: custom() })).rejects.toThrow(
      /serves/
    );
    setTenantProvider(custom());
    expect((await connectTenant(emptyIo())).token).toBe("bearer");
    const io = cliIo();
    expect((await connectTenant(io, { useConfigured: false })).tenantName).toBe("sandbox");
    expect(io.calls.filter((args) => args.includes("whoami"))).toHaveLength(1);
  });

  it("chooses context once, passes tenant to whoami, parses logs, and gives login hint", async () => {
    const calls: string[][] = [];
    const io: AicIo = {
      aic: async (args) => {
        calls.push([...args]);
        if (args.includes("ctx"))
          return {
            status: 0,
            stdout: JSON.stringify([{ name: "sandbox", base_url: baseUrl, current: true }]),
            stderr: "",
          };
        if (args.includes("whoami")) return { status: 0, stdout: "token\n", stderr: "" };
        return { status: 0, stdout: '[{"x":1}]', stderr: "" };
      },
      http: async () => response(200, ""),
    };
    const provider = aicCliProvider(io, { tenant: "sandbox" });
    await provider.getToken({ reason: "initial" });
    await provider.getToken({ reason: "rejected", rejected: "old" });
    expect(calls.filter((args) => args.includes("ctx"))).toHaveLength(1);
    expect(
      calls
        .filter((args) => args.includes("whoami"))
        .every(
          (args) =>
            args.includes("--no-prompt") &&
            args.slice(args.indexOf("--tenant") + 1)[0] === "sandbox"
        )
    ).toBe(true);
    expect(await provider.logs!.transaction("id", { waitMs: 1500 })).toEqual([{ x: 1 }]);
    expect(calls.at(-1)).toContain("--timeout");
    expect(calls.at(-1)).toContain("2");
    const failing = aicCliProvider({
      ...io,
      aic: async (args) =>
        args.includes("ctx")
          ? {
              status: 0,
              stdout: JSON.stringify([{ name: "sandbox", base_url: baseUrl, current: true }]),
              stderr: "",
            }
          : { status: 1, stdout: "", stderr: "locked" },
    });
    await expect(failing.getToken({ reason: "initial" })).rejects.toThrow(/aic login/);
  });

  it("refreshes authenticated 401s with rejected token and leaves anonymous calls alone", async () => {
    const seen: unknown[] = [];
    const provider: TenantProvider = {
      ...custom(),
      getToken: async (request) => {
        seen.push(request);
        return request.reason === "initial" ? "old" : "new";
      },
    };
    setTenantProvider(provider);
    const io = emptyIo();
    io.http = async (req) =>
      response(req.headerLines.some(([, v]) => v === "Bearer old") ? 401 : 200, "ok");
    const session = await connectTenant(io);
    const { amRequest } = await import("../../src/aic/tenant.ts");
    await amRequest(io, session, { method: "GET", path: "/x" });
    expect(seen).toEqual([{ reason: "initial" }, { reason: "rejected", rejected: "old" }]);
    seen.length = 0;
    await amRequest(io, session, { method: "POST", path: "/x", anonymous: true });
    expect(seen).toEqual([]);
  });
});

function response(status: number, body: string): HttpResponse {
  return { status, headers: [], body };
}
function emptyIo(): AicIo {
  return {
    aic: async () => ({ status: 0, stdout: "", stderr: "" }),
    http: async () => response(200, ""),
  };
}
function cliIo(): AicIo & { calls: string[][] } {
  const calls: string[][] = [];
  return {
    calls,
    aic: async (args) => {
      calls.push([...args]);
      return args.includes("ctx")
        ? {
            status: 0,
            stdout: JSON.stringify([{ name: "sandbox", base_url: baseUrl, current: true }]),
            stderr: "",
          }
        : { status: 0, stdout: "token", stderr: "" };
    },
    http: async () => response(200, ""),
  } as AicIo & { calls: string[][] };
}
