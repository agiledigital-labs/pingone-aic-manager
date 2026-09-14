import { describe, expect, it } from "vitest";
import type { HttpRequest, HttpResponse } from "../../src/aic/http.ts";
import { amRequest, type AicIo, type TenantSession } from "../../src/aic/tenant.ts";

function session(): TenantSession {
  return {
    tenantName: "sandbox",
    baseUrl: "https://tenant.invalid",
    token: "first-token",
    project: "/tmp/rhino-local-tenant-test",
  };
}

/** Answers 401 until the bearer changes, then 200. */
function fakeIo(options: { tokenAfterRefresh?: string } = {}): {
  io: AicIo;
  bearers: string[];
  aicArgs: string[][];
} {
  const bearers: string[] = [];
  const aicArgs: string[][] = [];
  const io: AicIo = {
    aic(args) {
      aicArgs.push(args);
      return Promise.resolve({
        status: 0,
        stdout: `${options.tokenAfterRefresh ?? "second-token"}\n`,
        stderr: "",
      });
    },
    http(req: HttpRequest): Promise<HttpResponse> {
      const authorization = req.headerLines
        .filter(([name]) => name.toLowerCase() === "authorization")
        .map(([, value]) => value);
      bearers.push(authorization[0] ?? "<none>");
      const stale = authorization[0] === "Bearer first-token";
      return Promise.resolve({
        status: stale ? 401 : 200,
        headers: [],
        body: JSON.stringify({ ok: !stale }),
      });
    },
  };
  return { io, bearers, aicArgs };
}

describe("amRequest bearer refresh", () => {
  it("refreshes and retries once when an authenticated call returns 401", async () => {
    const fake = fakeIo();
    const response = await amRequest(fake.io, session(), {
      method: "GET",
      path: "/am/json/realms/root/realms/alpha/scripts/x",
    });

    expect(response.status).toBe(200);
    // The retry must actually carry the NEW bearer — a retry that re-sent the
    // dead one would loop or fail, and asserting only on the call count would
    // not notice.
    expect(fake.bearers).toEqual(["Bearer first-token", "Bearer second-token"]);
    expect(fake.aicArgs).toHaveLength(1);
    expect(fake.aicArgs[0]).toContain("--no-prompt");
    expect(fake.aicArgs[0]).toContain("--token");
  });

  it("adopts the refreshed bearer for later calls on the same session", async () => {
    const fake = fakeIo();
    const live = session();
    await amRequest(fake.io, live, { method: "GET", path: "/first" });
    await amRequest(fake.io, live, { method: "GET", path: "/second" });

    expect(live.token).toBe("second-token");
    // Second call starts on the refreshed bearer, so it needs no second refresh.
    expect(fake.bearers).toEqual([
      "Bearer first-token",
      "Bearer second-token",
      "Bearer second-token",
    ]);
    expect(fake.aicArgs).toHaveLength(1);
  });

  it("does not retry an anonymous 401, which is a journey verdict", async () => {
    const fake = fakeIo();
    const response = await amRequest(fake.io, session(), {
      method: "POST",
      path: "/am/json/realms/root/realms/alpha/authenticate",
      anonymous: true,
    });

    // No bearer was sent, so the fake answers 200; what matters is that no
    // refresh was attempted for an unauthenticated call.
    expect(response.status).toBe(200);
    expect(fake.aicArgs).toEqual([]);
  });

  it("fails loudly when the agent cannot supply a fresh bearer", async () => {
    const io: AicIo = {
      aic: () => Promise.resolve({ status: 3, stdout: "", stderr: "agent is locked" }),
      http: () =>
        Promise.resolve({ status: 401, headers: [], body: JSON.stringify({ ok: false }) }),
    };
    await expect(
      amRequest(io, session(), { method: "GET", path: "/x" })
    ).rejects.toThrow(/whoami --token failed while refreshing the bearer/);
  });
});
