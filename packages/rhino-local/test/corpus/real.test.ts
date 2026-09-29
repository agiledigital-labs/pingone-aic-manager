import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { judge } from "../../src/case/index.ts";
import type { CallbackEffect } from "../../src/case/types.ts";
import type { RunCaseOptions } from "../../src/bindings/run.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { blockedCases, gapCases, realCases, runnableCases } from "../../cases/real/index.ts";
import { PROBE_SOURCE_NAME, type RealEntry } from "../../cases/real/load.ts";

function runOptions(entry: RealEntry, sourceName: string): RunCaseOptions {
  // RSA key generation (binding-utils-subtle) runs past 5 s on a loaded CI
  // runner; locally every case finishes in well under one.
  const options: RunCaseOptions = { sourceName, timeoutMs: 20_000 };
  if (entry.libraries !== undefined) {
    options.libraries = entry.libraries;
  }
  return options;
}

describe("real scripted-decision corpus", { timeout: 30_000 }, () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  it("registers every copied probe as a case", () => {
    expect(realCases.length).toBeGreaterThan(40);
    expect(runnableCases.length + blockedCases.length + gapCases.length).toBe(realCases.length);
  });

  it("every blocked case names the missing method and the throw", () => {
    for (const entry of blockedCases) {
      expect(entry.blocked?.method, entry.kase.name).toBeTruthy();
      expect(entry.blocked?.throw, entry.kase.name).toBeTruthy();
    }
  });

  describe("cases that can produce a verdict today", () => {
    if (runnableCases.length === 0) {
      it("none — every real script is blocked on a missing binding or a parse error", () => {
        expect(runnableCases).toEqual([]);
      });
    }
    for (const entry of runnableCases) {
      it(entry.kase.name, async () => {
        const result = await runCase(runner, entry.kase, runOptions(entry, PROBE_SOURCE_NAME));
        expect(result.verdict.summary, result.verdict.summary).toBe("");
        expect(result.verdict.pass).toBe(true);
      });
    }
  });

  describe("known local-vs-AIC gaps", () => {
    for (const entry of gapCases) {
      const gap = entry.gap;
      if (gap === undefined) {
        continue;
      }
      it(`${entry.kase.name} — differs on ${gap.differs.join(", ")}`, async () => {
        expect(gap.reason, entry.kase.name).toBeTruthy();
        const result = await runCase(runner, entry.kase, runOptions(entry, PROBE_SOURCE_NAME));
        const local = emitted(result.effects.callbacks);
        const live = emitted(entry.kase.expect.callbacks ?? []);
        // The payload's keys are exactly AIC's plus the declared gap — an
        // extra local key, or a declared key the script never emitted, fails.
        expect(Object.keys(local).sort(), `${entry.kase.name} payload keys`).toEqual(
          [...new Set([...Object.keys(live), ...gap.differs])].sort()
        );
        for (const key of Object.keys(live)) {
          if (gap.differs.includes(key)) {
            continue;
          }
          expect(local[key], `${entry.kase.name}.${key} matches AIC`).toEqual(live[key]);
        }
        // Everything outside the payload — outcome, state, every other
        // callback, openidm, http, logs — is judged as a passing case would
        // be, with only the payload's own value taken from the local run.
        const verdict = judge(
          {
            ...entry.kase,
            expect: {
              ...entry.kase.expect,
              callbacks: withPayload(entry.kase.expect.callbacks ?? [], result.effects.callbacks),
            },
          },
          result.effects
        );
        expect(verdict.summary, `${entry.kase.name} outside the gap`).toBe("");
        expect(verdict.pass).toBe(true);
        for (const key of gap.differs) {
          if (key in live) {
            // The live value is committed: the gap must still be a gap.
            expect(
              withoutIdentityHashes(local[key]),
              `${entry.kase.name}.${key} now matches AIC — the gap closed; drop it from differs`
            ).not.toEqual(withoutIdentityHashes(live[key]));
          }
          // Pinned, so a change here — the gap closing, or the harness
          // drifting further — fails and gets looked at.
          expect(withoutIdentityHashes(local[key])).toMatchSnapshot(key);
        }
      });
    }
  });

  describe("blocked cases still throw the named gap", () => {
    for (const entry of blockedCases) {
      const blocked = entry.blocked;
      if (blocked === undefined) {
        continue;
      }
      it(`${entry.kase.name} — ${blocked.method}`, async () => {
        await expect(
          runCase(runner, entry.kase, runOptions(entry, entry.kase.name))
        ).rejects.toThrow(blocked.throw);
      });
    }
  });
});

/** The payload of the HiddenValueCallback every probe fixture emits. */
function emitted(
  callbacks: ReadonlyArray<{ type: string; value?: unknown }>
): Record<string, unknown> {
  const hidden = callbacks.find((callback) => callback.type === "HiddenValueCallback");
  if (typeof hidden?.value !== "string") {
    throw new Error(`no HiddenValueCallback payload in ${JSON.stringify(callbacks)}`);
  }
  return JSON.parse(hidden.value) as Record<string, unknown>;
}

/** `expected` with its HiddenValueCallback's value replaced by the one in `actual`. */
function withPayload(
  expected: readonly CallbackEffect[],
  actual: readonly CallbackEffect[]
): CallbackEffect[] {
  const payload = actual.find((callback) => callback.type === "HiddenValueCallback");
  const value = payload?.value;
  if (value === undefined) {
    throw new Error("the local run emitted no HiddenValueCallback");
  }
  return expected.map((callback) =>
    callback.type === "HiddenValueCallback" ? { ...callback, value } : callback
  );
}

/**
 * `org.mozilla.javascript.Undefined@421faab1` differs on every run, locally and
 * live; a `rhino-local-mocks.cjs#<n>` line moves with every edit to the mocks.
 * Neither is part of what a gap pins.
 */
function withoutIdentityHashes(value: unknown): unknown {
  if (value === undefined) {
    return value;
  }
  return JSON.parse(
    JSON.stringify(value)
      .replace(/@[0-9a-f]{4,8}\b/g, "@<hash>")
      .replace(/rhino-local-mocks\.cjs#\d+/g, "rhino-local-mocks.cjs#<line>")
  ) as unknown;
}
