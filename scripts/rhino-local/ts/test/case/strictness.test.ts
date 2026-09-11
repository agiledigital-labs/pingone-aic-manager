import { describe, expect, it } from "vitest";
import { judge } from "../../src/case/index.ts";
import { makeCase, makeEffects } from "./helpers.ts";

describe("per-channel strictness — openidm", () => {
  it("fails an undeclared write (would pass if extras were ignored)", () => {
    const kase = makeCase({
      expect: { outcome: "true", openidm: [] },
    });
    const verdict = judge(
      kase,
      makeEffects({
        openidm: [
          {
            method: "create",
            resource: "managed/alpha_user/alice",
            body: { userName: "alice" },
          },
        ],
      })
    );
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches).toEqual([
      {
        channel: "openidm",
        path: 'create managed/alpha_user/alice body={"userName":"alice"}',
        expected: "(none)",
        actual: 'create managed/alpha_user/alice body={"userName":"alice"}',
        message:
          'openidm: undeclared write create managed/alpha_user/alice body={"userName":"alice"}',
      },
    ]);
  });

  it("also fails an undeclared write when the case never mentioned openidm", () => {
    const kase = makeCase({ expect: { outcome: "true" } });
    const verdict = judge(
      kase,
      makeEffects({
        openidm: [{ method: "delete", resource: "managed/alpha_user/alice" }],
      })
    );
    expect(verdict.pass).toBe(false);
    expect(verdict.mismatches[0]?.message).toMatch(/^openidm: undeclared write/);
  });

  it("passes a declared write", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        openidm: [
          {
            method: "create",
            resource: /managed\/alpha_user\//,
            body: { userName: "alice" },
          },
        ],
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        openidm: [
          {
            method: "create",
            resource: "managed/alpha_user/alice",
            body: { userName: "alice" },
          },
        ],
      })
    );
    expect(verdict.pass).toBe(true);
  });

  it("requires an explicit opt-out to allow undeclared writes", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        openidm: [
          { method: "update", resource: "managed/alpha_user/alice" },
        ],
        allowUndeclared: { openidmWrites: true },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        openidm: [
          { method: "update", resource: "managed/alpha_user/alice" },
          { method: "delete", resource: "managed/alpha_user/bob" },
        ],
      })
    );
    expect(verdict.pass).toBe(true);
  });

  it("ignores undeclared reads by default", () => {
    const kase = makeCase({ expect: { outcome: "true" } });
    const verdict = judge(
      kase,
      makeEffects({
        openidm: [{ method: "read", resource: "managed/alpha_user/alice" }],
      })
    );
    expect(verdict.pass).toBe(true);
  });

  it("fails undeclared reads when the case tightens the channel", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        allowUndeclared: { openidmReads: false },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        openidm: [{ method: "query", resource: "managed/alpha_user" }],
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "openidm",
        path: "query managed/alpha_user",
        expected: "(none)",
        actual: "query managed/alpha_user",
        message: "openidm: undeclared read query managed/alpha_user",
      },
    ]);
  });

  it("treats action as a write", () => {
    const kase = makeCase({ expect: { outcome: "true" } });
    const verdict = judge(
      kase,
      makeEffects({
        openidm: [
          {
            method: "action",
            resource: "managed/alpha_user/alice",
            actionName: "resetPassword",
          },
        ],
      })
    );
    expect(verdict.mismatches[0]?.message).toBe(
      'openidm: undeclared write action managed/alpha_user/alice action="resetPassword"'
    );
  });
});

describe("per-channel strictness — http", () => {
  it("fails an undeclared request", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        http: [{ url: /\/verify$/, times: 1 }],
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        http: [
          { method: "GET", url: "https://example.com/verify" },
          { method: "POST", url: "https://example.com/other" },
        ],
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "http",
        path: "POST https://example.com/other",
        expected: "(none)",
        actual: "POST https://example.com/other",
        message: "http: undeclared request POST https://example.com/other",
      },
    ]);
  });

  it("fails when the expected request never happened", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        http: [{ url: /\/verify$/, method: "GET" }],
      },
    });
    const verdict = judge(kase, makeEffects());
    expect(verdict.mismatches).toEqual([
      {
        channel: "http",
        path: "GET /\\/verify$/",
        expected: "1",
        actual: "0",
        message: "http: expected 1 request matching GET /\\/verify$/, actual 0",
      },
    ]);
  });

  it("requires an explicit opt-out to allow undeclared requests", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        http: [{ url: /\/verify$/ }],
        allowUndeclared: { http: true },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        http: [
          { method: "GET", url: "https://example.com/verify" },
          { method: "GET", url: "https://example.com/metrics" },
        ],
      })
    );
    expect(verdict.pass).toBe(true);
  });
});

describe("per-channel strictness — logs", () => {
  it("passes an undeclared log line (would fail if logs were fail-closed)", () => {
    const kase = makeCase({ expect: { outcome: "true" } });
    const verdict = judge(
      kase,
      makeEffects({
        logs: [{ level: "info", message: "checking email for alice" }],
      })
    );
    expect(verdict.pass).toBe(true);
    expect(verdict.mismatches).toEqual([]);
  });

  it("still requires listed log lines to appear", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        logs: [{ level: "error", message: /denied/ }],
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        logs: [{ level: "info", message: "checking email for alice" }],
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "logs",
        path: "error /denied/",
        expected: "1",
        actual: "0",
        message: "logs: expected 1 line matching error /denied/, actual 0",
      },
    ]);
  });

  it("fails extra log lines only when the case tightens the channel", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        logs: [{ message: "ok" }],
        allowUndeclared: { logs: false },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        logs: [
          { level: "info", message: "ok" },
          { level: "debug", message: "noise" },
        ],
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "logs",
        path: 'debug "noise"',
        expected: "(none)",
        actual: 'debug "noise"',
        message: 'logs: undeclared line debug "noise"',
      },
    ]);
  });
});

describe("per-channel strictness — callbacks", () => {
  it("fails an undeclared callback", () => {
    const kase = makeCase({
      expect: { outcome: "true", callbacks: [] },
    });
    const verdict = judge(
      kase,
      makeEffects({
        callbacks: [{ type: "NameCallback", prompt: "User Name" }],
      })
    );
    expect(verdict.mismatches).toEqual([
      {
        channel: "callbacks",
        path: "[0]",
        expected: "(none)",
        actual: '{"type":"NameCallback","prompt":"User Name"}',
        message: "callbacks: [0] differed",
      },
    ]);
  });

  it("compares callbacks in order", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        callbacks: [
          { type: "NameCallback", prompt: "User Name" },
          { type: "PasswordCallback" },
        ],
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        callbacks: [
          { type: "NameCallback", prompt: "User Name" },
          { type: "PasswordCallback" },
        ],
      })
    );
    expect(verdict.pass).toBe(true);
  });

  it("allows extra callbacks only with an explicit opt-out", () => {
    const kase = makeCase({
      expect: {
        outcome: "true",
        callbacks: [{ type: "NameCallback" }],
        allowUndeclared: { callbacks: true },
      },
    });
    const verdict = judge(
      kase,
      makeEffects({
        callbacks: [
          { type: "TextOutputCallback", message: "Welcome" },
          { type: "NameCallback" },
        ],
      })
    );
    expect(verdict.pass).toBe(true);
  });
});
