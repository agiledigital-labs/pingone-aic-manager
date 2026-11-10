import { judge, validateCase } from "../case/index.ts";
import type { Case, CompleteRecordedEffects, Verdict } from "../case/types.ts";
import type { JobRequest, JobResponse } from "../protocol.ts";
import type { RhinoRunner } from "../runner.ts";
import { parseHarvest } from "./harvest.ts";
import { casePreamble, harvestJob, type MockPreambleOptions } from "./preamble.ts";
import {
  accessTokenClassAllowList,
  configProviderClassAllowList,
  decisionNodeClassAllowList,
  oidcClaimsClassAllowList,
  socialIdpClassAllowList,
  validateScopeClassAllowList,
} from "./allowlist.ts";
import { checkSeededManaged } from "../profile/seed.ts";
import type { EnvProfile } from "../profile/types.ts";

export interface RunCaseOptions {
  timeoutMs?: number;
  sourceName?: string;
  /** AM library script bodies keyed by `require()` id. See mockPreamble. */
  libraries?: Record<string, string>;
  /**
   * Pulled environment schema. Supplying one turns on strict fixture checking
   * and lets a declared-but-unseeded object type read as empty (AIC's `null`)
   * instead of a missing-fixture throw.
   */
  profile?: EnvProfile;
  /**
   * Install AM's Java class shutter. On by default: a scripted-decision case
   * should run behind the same allow-list the tenant enforces, or the harness
   * is more permissive than AIC and green means nothing. Pass `false` only to
   * demonstrate the difference.
   */
  classShutter?: boolean;
}

export interface CaseRun {
  effects: CompleteRecordedEffects;
  verdict: Verdict;
  response: JobResponse;
}

/**
 * A case whose run did not complete. When a harness fault failed a run the
 * script itself completed, `faultedEffects` is what the script did around the
 * refusal — for comparing the parts the fault did not touch, never for a
 * verdict on the case.
 */
export class CaseRunError extends Error {
  readonly response: JobResponse;
  readonly faultedEffects: CompleteRecordedEffects | undefined;
  constructor(message: string, response: JobResponse) {
    super(message);
    this.name = "CaseRunError";
    this.response = response;
    this.faultedEffects =
      typeof response.faultedValue === "string" ? parseHarvest(response.faultedValue) : undefined;
  }
}

/**
 * Eval a case against the local Rhino runner: mocks as preamble, author script
 * as source, harvest appended so line numbers do not shift. The preamble goes
 * as {@link casePreamble}'s parts, so the runner compiles the mocks and each
 * library once per JVM instead of once per case.
 */
export async function runCase(
  runner: RhinoRunner,
  input: Case,
  options: RunCaseOptions = {}
): Promise<CaseRun> {
  const kase = validateCase(input);
  if (options.profile !== undefined) {
    // Fail before the JVM sees the case: a fixture that names a property the
    // environment does not define is a test bug, and saying so here names the
    // case rather than surfacing as a puzzling mismatch downstream.
    checkSeededManaged(options.profile, kase.given.managed, kase.name, kase.given.identityCustomAttrs);
  }
  const request: JobRequest = {
    ...harvestJob(kase.script, kase.context),
    sourceName: options.sourceName ?? kase.name,
    ...casePreamble(kase.given, preambleOptions(options, kase)),
    preambleName: "rhino-local-mocks.cjs",
  };
  if (options.timeoutMs !== undefined) {
    request.timeoutMs = options.timeoutMs;
  }
  // The legacy evaluator does not enforce the next-gen decision node's
  // allow-list: `legacy-es2015-globals` records `new java.util.HashMap()`
  // succeeding on a live legacy run, where next-gen hides it. Installing the
  // next-gen list there would invent a restriction the tenant does not apply.
  const shutterApplies =
    options.classShutter !== false && kase.given.engine !== "legacy";
  if (shutterApplies) {
    // Each next-gen context has its own allow-list (docs/api/bindings/).
    request.classAllowList =
      kase.context === "OAUTH2_ACCESS_TOKEN_MODIFICATION"
        ? accessTokenClassAllowList()
        : kase.context === "OIDC_CLAIMS"
          ? oidcClaimsClassAllowList()
          : kase.context === "SOCIAL_IDP_PROFILE_TRANSFORMATION"
            ? socialIdpClassAllowList()
            : kase.context === "CONFIG_PROVIDER_NODE"
              ? configProviderClassAllowList()
              : kase.context === "OAUTH2_VALIDATE_SCOPE"
                ? validateScopeClassAllowList()
                : decisionNodeClassAllowList();
  }
  const response = await runner.eval(request);
  if (response.outcome !== "ok") {
    const detail = response.error?.message ?? response.outcome;
    throw new CaseRunError(
      `rhino-local: case ${JSON.stringify(kase.name)} ${response.outcome}: ${detail}`,
      response
    );
  }
  if (typeof response.value !== "string") {
    throw new Error(
      `rhino-local: harvest returned ${response.valueKind}, not a JSON string`
    );
  }
  const effects = parseHarvest(response.value);
  return { effects, verdict: judge(kase, effects), response };
}

function preambleOptions(options: RunCaseOptions, kase: Case): MockPreambleOptions {
  const out: MockPreambleOptions = {};
  if (kase.context !== undefined) {
    out.context = kase.context;
  }
  if (options.libraries !== undefined) {
    out.libraries = options.libraries;
  }
  if (options.profile !== undefined) {
    out.profile = options.profile;
  }
  return out;
}
