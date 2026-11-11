import { judge, validateCase } from "../case/index.ts";
import type { Case, CompleteRecordedEffects, LibraryCallerContext, Verdict } from "../case/types.ts";
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
  mayActClassAllowList,
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
  const executed = libraryExecution(kase);
  if (options.profile !== undefined) {
    // Fail before the JVM sees the case: a fixture that names a property the
    // environment does not define is a test bug, and saying so here names the
    // case rather than surfacing as a puzzling mismatch downstream.
    checkSeededManaged(options.profile, kase.given.managed, kase.name, kase.given.identityCustomAttrs);
  }
  const request: JobRequest = {
    ...harvestJob(executed.script, kase.context),
    sourceName: options.sourceName ?? kase.name,
    ...casePreamble(executed.given, preambleOptions(options, kase)),
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
    // A library runs inside its caller's shutter: the decision-node list
    // constructs HashSet, and the four OAuth lists refuse it.
    request.classAllowList = classAllowListFor(kase);
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

/**
 * The allow-list for this case. A LIBRARY case uses the list of
 * `given.library.callerContext` (default the decision node). Evaluate scope
 * is a caller, not a case context, and its descriptor's `allowLists` is the
 * same three entries as the other three OAuth callers; the library e2e test
 * locks those four lists together, so evaluate scope uses the may-act list.
 */
function classAllowListFor(kase: Case): string[] {
  switch (kase.context) {
    case "LIBRARY":
      return libraryCallerClassAllowList(kase.given.library?.callerContext);
    case "OAUTH2_ACCESS_TOKEN_MODIFICATION":
      return accessTokenClassAllowList();
    case "OIDC_CLAIMS":
      return oidcClaimsClassAllowList();
    case "SOCIAL_IDP_PROFILE_TRANSFORMATION":
      return socialIdpClassAllowList();
    case "CONFIG_PROVIDER_NODE":
      return configProviderClassAllowList();
    case "OAUTH2_VALIDATE_SCOPE":
      return validateScopeClassAllowList();
    case "OAUTH2_MAY_ACT":
      return mayActClassAllowList();
    case "SCRIPTED_DECISION_NODE":
    case undefined:
      return decisionNodeClassAllowList();
    default: {
      const unexpected: never = kase.context;
      throw new Error(`rhino-local: no class allow-list for ${String(unexpected)}`);
    }
  }
}

function libraryCallerClassAllowList(caller: LibraryCallerContext | undefined): string[] {
  switch (caller) {
    case "OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN":
      return accessTokenClassAllowList();
    case "OAUTH2_VALIDATE_SCOPE_NEXT_GEN":
      return validateScopeClassAllowList();
    case "OAUTH2_MAY_ACT_NEXT_GEN":
    case "OAUTH2_EVALUATE_SCOPE_NEXT_GEN":
      return mayActClassAllowList();
    case "AUTHENTICATION_TREE_DECISION_NODE":
    case undefined:
      return decisionNodeClassAllowList();
    default: {
      const unexpected: never = caller;
      throw new Error(`rhino-local: no class allow-list for library caller ${String(unexpected)}`);
    }
  }
}

/**
 * The id the harvest require()s. Not a script name an author can collide
 * with: given.libraries is checked for it.
 */
const LIBRARY_SUBJECT_ID = "__rhinoLocalSubject";

/**
 * A LIBRARY case's script is the library body. The runner executes an empty
 * caller whose harvest require()s that body through the existing loader, so
 * the library closes over library scope rather than over a decision node.
 */
function libraryExecution(kase: Case): { script: string; given: Case["given"] } {
  if (kase.context !== "LIBRARY" || kase.given.library === undefined) {
    return { script: kase.script, given: kase.given };
  }
  if (Object.prototype.hasOwnProperty.call(kase.given.libraries ?? {}, LIBRARY_SUBJECT_ID)) {
    throw new Error(
      `rhino-local: given.libraries.${LIBRARY_SUBJECT_ID} is reserved for the case script`
    );
  }
  return {
    script: "",
    given: {
      ...kase.given,
      library: { ...kase.given.library, id: LIBRARY_SUBJECT_ID },
      libraries: { ...(kase.given.libraries ?? {}), [LIBRARY_SUBJECT_ID]: kase.script },
    },
  };
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
