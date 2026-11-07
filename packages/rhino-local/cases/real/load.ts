import { readFileSync } from "node:fs";
import { join } from "node:path";
import { defineCase } from "../../src/case/index.ts";
import type {
  CallbackEffect,
  Case,
  Expect,
  Given,
} from "../../src/case/types.ts";
import type { EnvProfile } from "../../src/profile/types.ts";
import { repoRoot } from "../../src/paths.ts";

/** Load an AM LIBRARY fixture body for `require()` by script name. */
export function librarySource(file: string): string {
  return readFileSync(
    join(repoRoot, "scripts/rhino-script-tester/fixtures", file),
    "utf8"
  );
}

/**
 * The name `scripts/rhino-script-tester/update-script.sh` gives the tenant
 * script every probe runs as, so a runtime error's `(name#line)` suffix reads
 * the same locally as live.
 */
export const PROBE_SOURCE_NAME = "AIC Rhino Let Probe";

/**
 * Load a case's script from its origin fixture — the file the tenant runs —
 * rather than a copy, so the local run cannot drift from the live one and an
 * error's line number is the line AIC reports.
 */
export function loadOrigin(origin: string, rewrites: readonly Rewrite[] = []): string {
  let source = readFileSync(join(repoRoot, origin), "utf8");
  for (const [pattern, replacement] of rewrites) {
    const hits = source.match(new RegExp(pattern.source, `${pattern.flags.replace("g", "")}g`));
    if (hits?.length !== 1) {
      throw new Error(
        `rhino-local: rewrite ${String(pattern)} matched ${hits?.length ?? 0} times in ${origin}, not once — the fixture changed`
      );
    }
    source = source.replace(pattern, replacement);
  }
  return source;
}

/**
 * One substitution applied to an origin fixture before it runs, matched by
 * the fixture's **structure** (`var PROBE_USER = "…"`), never by the value it
 * replaces: identity probes name tenant users, and this is how a case points
 * them at placeholder records without repeating a tenant identifier here.
 * Each must match exactly once, so a fixture edit fails loudly.
 */
export type Rewrite = readonly [pattern: RegExp, replacement: string];

/**
 * A measured difference between the local harness and AIC, kept visible
 * rather than papered over. Every payload key **not** listed must still equal
 * the committed live payload. Each listed key's local value is pinned by a
 * snapshot, and where its live value is committed too (it is left out only
 * when it cannot be sanitised) the local value must still differ from it. So
 * closing the gap, or the local output drifting, fails and gets looked at.
 */
export interface KnownGap {
  /** What AIC does, what the harness does instead, and what would close it. */
  reason: string;
  /** Top-level payload keys that differ from the live run today. */
  differs: readonly string[];
  /**
   * Harness fault ids (E19) the local run hits. The probe catches each of these
   * refusals to record a payload key, but a harness fault fails the run whether
   * or not the script catches it, so a case listing faults produces no payload
   * to compare: the suite asserts the run fails with exactly these fault ids
   * instead. Emulating the behaviour (or seeding the fixture) removes the id.
   */
  faults?: readonly string[];
}

/** HiddenValueCallback the probe fixtures emit on a successful live run. */
export function hiddenValue(payload: Record<string, unknown>): CallbackEffect[] {
  return [
    {
      type: "HiddenValueCallback",
      id: "result",
      value: JSON.stringify(payload),
    },
  ];
}

export interface BlockedBy {
  /** Binding method path, e.g. `callbacks.isEmpty`. */
  method: string;
  /**
   * Exact `Error.message` the JVM runner reported. Compared as a substring of
   * `runCase`'s thrown error so a later overlay that implements the method
   * fails this assertion and unblocks the case.
   */
  throw: string;
}

export interface RealEntry {
  kase: Case;
  /** Path of the original fixture, relative to the repo root. */
  origin: string;
  kind: "next-gen" | "legacy";
  /**
   * Present when the case cannot yet produce a verdict. The suite stays green
   * by skipping the pass assertion and instead asserting this throw still
   * happens.
   */
  blocked?: BlockedBy;
  /** AM library script bodies keyed by `require()` id. */
  libraries?: Record<string, string>;
  /** Present when local and live measurably differ; see {@link KnownGap}. */
  gap?: KnownGap;
  /** The environment the case runs against, when it depends on which managed types exist. */
  profile?: EnvProfile;
  /** Hooks the case's live run exercised; see {@link LiveHooks}. */
  liveHooks?: LiveHooks;
}

/**
 * Object hooks of the case's own profile that its **live run** ran, so the compared payload keys
 * already are the measurement of what those hooks did to the script's writes. The corpus runner
 * subtracts them when it computes this case's lane write faults ({@link profileForRun}); the
 * profile itself stays true, and every hook not named here still faults.
 *
 * Corpus-only by construction: it lives on `RealEntry`, never on `Given` or `RunCaseOptions`, so a
 * client case cannot reach it. {@link realCase} refuses it unless every named hook is in the case's
 * own profile (it cannot exempt a hook the tenant lacks) and the case carries a live payload (a
 * HiddenValueCallback the probe emitted, and no `blocked`). It exempts hooks only, never a policy.
 * {@link profileForRun} repeats those checks on the entry it is given.
 *
 * Known limit: the checks prove the case has *a* live payload, not that the live run wrote to the
 * types whose hooks it names — an unrelated read-only probe's payload would satisfy them. Binding
 * each named hook to a compared write observation is not built; that link rests on review.
 */
export interface LiveHooks {
  /** The live probe run the payload comes from, `YYYY-MM-DD`. */
  readonly probed: string;
  /** Managed type name → object hook names, each declared in the profile's `hooks` for that type. */
  readonly hooks: Readonly<Record<string, readonly string[]>>;
}

/** `YYYY-MM-DD` naming a day that exists (`2026-13-45` does not). */
function isCalendarDate(value: unknown): boolean {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const date = new Date(`${value}T00:00:00.000Z`);
  return !Number.isNaN(date.getTime()) && date.toISOString().slice(0, 10) === value;
}

/** Refuse a {@link LiveHooks} the case cannot back with its own profile and live payload. */
function checkLiveHooks(name: string, live: LiveHooks, profile: EnvProfile | undefined, expect: Expect, blocked: BlockedBy | undefined): void {
  const refuse = (why: string): never => {
    throw new Error(`rhino-local: case ${JSON.stringify(name)}: liveHooks ${why}`);
  };
  if (!isCalendarDate(live.probed)) refuse(`names no probe date (got ${JSON.stringify(live.probed)})`);
  const payload = (expect.callbacks ?? []).find((callback) => callback.type === "HiddenValueCallback");
  if (blocked !== undefined || typeof payload?.value !== "string") {
    refuse("needs a live payload: a HiddenValueCallback the probe emitted, on a case that is not blocked");
  }
  if (profile === undefined) refuse("needs the case's own profile");
  for (const [type, hooks] of Object.entries(live.hooks)) {
    const declared = profile!.objects[type]?.hooks ?? [];
    for (const hook of hooks) {
      if (!declared.includes(hook)) refuse(`names ${type}.${hook}, which the case's profile does not declare`);
    }
  }
}

/**
 * The profile the corpus runs a case with: its own, minus the object hooks its live run exercised
 * ({@link LiveHooks}). Only the lane's hook faults read `hooks`, so nothing else changes.
 *
 * The acknowledgement is checked again here, against the entry as it is now (sol-r1 P2-2): this is
 * what the corpus runner consumes, and a `RealEntry` built by spreading another, or edited after
 * {@link realCase} built it, never went through that check.
 */
export function profileForRun(entry: RealEntry): EnvProfile | undefined {
  const profile = entry.profile;
  if (entry.liveHooks === undefined) return profile;
  checkLiveHooks(entry.kase.name, entry.liveHooks, profile, entry.kase.expect, entry.blocked);
  if (profile === undefined) return profile;
  const live = entry.liveHooks.hooks;
  return { ...profile, objects: Object.fromEntries(Object.entries(profile.objects).map(([type, object]) => [type,
    live[type] === undefined ? object : { ...object, hooks: (object.hooks ?? []).filter((hook) => !live[type]!.includes(hook)) }])) };
}

export function realCase(input: {
  name: string;
  kind: "nextgen" | "legacy";
  origin: string;
  given?: Given;
  expect: Expect;
  blocked?: BlockedBy;
  libraries?: Record<string, string>;
  rewrites?: readonly Rewrite[];
  gap?: KnownGap;
  profile?: EnvProfile;
  liveHooks?: LiveHooks;
}): RealEntry {
  const init: {
    name: string;
    script: string;
    given?: Given;
    expect: Expect;
  } = {
    name: input.name,
    script: loadOrigin(input.origin, input.rewrites),
    expect: input.expect,
  };
  let given: Given | undefined = input.given;
  if (input.kind === "legacy") {
    given = { ...(given ?? {}), engine: "legacy" };
  }
  if (given !== undefined) {
    init.given = given;
  }
  const entry: RealEntry = {
    kase: defineCase(init),
    origin: input.origin,
    kind: input.kind === "legacy" ? "legacy" : "next-gen",
  };
  if (input.blocked !== undefined) {
    entry.blocked = input.blocked;
  }
  if (input.libraries !== undefined) {
    entry.libraries = input.libraries;
  }
  if (input.gap !== undefined) {
    entry.gap = input.gap;
  }
  if (input.profile !== undefined) {
    entry.profile = input.profile;
  }
  if (input.liveHooks !== undefined) {
    checkLiveHooks(input.name, input.liveHooks, input.profile, input.expect, input.blocked);
    entry.liveHooks = input.liveHooks;
  }
  return entry;
}
