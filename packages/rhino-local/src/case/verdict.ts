import { parseIdentityCustomAttrs, parseIdentityCustomAttrsOwnedKeys } from "./identity.ts";
import { deepEqual, matchesPattern } from "./equal.ts";
import { matchesValue } from "./matcher.ts";
import { isPortable } from "./portable.ts";
import {
  ALLOW_UNDECLARED_CHANNELS,
  CHANNELS,
  DEFAULT_ALLOW_UNDECLARED,
  LOG_LEVELS,
  OPENIDM_METHODS,
  OPENIDM_WRITE_METHODS,
  STATE_CHANNELS,
} from "./types.ts";
import { TOKEN_VALUE_KEYS } from "./context.ts";
import type {
  AccessTokenEffect,
  OidcClaimsEffect,
  SocialIdpEffect,
  ConfigProviderEffect,
  ValidateScopeEffect,
  AccessTokenState,
  AllowUndeclared,
  AllowUndeclaredChannel,
  CallbackEffect,
  Case,
  Channel,
  EvidenceChannel,
  Expect,
  ExpectedValue,
  HttpEffect,
  HttpExpect,
  IdentityWriteEffect,
  IdentityWriteExpect,
  JsonObject,
  JsonValue,
  LogEffect,
  LogExpect,
  LogLevel,
  Mismatch,
  OpenidmEffect,
  OpenidmExpect,
  OpenidmMethod,
  CompleteRecordedEffects,
  RecordingEvidence,
  StateBucket,
  StateChannel,
  StateDiff,
  StateMutation,
  UnbucketedStateMutation,
  Unverified,
  Verdict,
} from "./types.ts";
import { defineOwn, formatValue, isPlainObject, parseIdentityWriteEffect, parseJsonObject, parseJsonValue } from "./util.ts";
import { validateCase } from "./validate.ts";

/**
 * Channels 0.1.2 did not record. 0.1.2 accepted `putSessionProperty` and
 * `identity.store()` and recorded neither, so effects without these keys are
 * parsed as UNOBSERVED rather than empty: an empty bucket would assert "the
 * script changed nothing" about a run that may have.
 */
const LATER_CHANNELS: ReadonlySet<Channel> = new Set<Channel>(["sessionProperties", "identityWrites"]);

const WRITE_METHODS: ReadonlySet<string> = new Set(OPENIDM_WRITE_METHODS);
const OPENIDM_METHOD_SET: ReadonlySet<string> = new Set(OPENIDM_METHODS);
const LOG_LEVEL_SET: ReadonlySet<string> = new Set(LOG_LEVELS);
const CHANNEL_SET: ReadonlySet<string> = new Set(CHANNELS);
const STATE_CHANNEL_SET: ReadonlySet<string> = new Set(STATE_CHANNELS);

type Strictness = { [K in AllowUndeclaredChannel]: boolean };

/**
 * Pure function: (case, recorded effects) → pass/fail with a readable
 * explanation of each difference. Does not run a script.
 */
export function judge(input: unknown, effects: unknown): Verdict {
  const kase = validateCase(input);
  const recorded = parseEffects(effects);
  const strictness = resolveStrictness(kase.expect);
  const evidence = recorded.evidence ?? exactEvidence();
  const unobserved = new Set(evidence.unobservedChannels);
  const reconciled = reconcileUnbucketedState(
    kase.expect,
    evidence.unbucketedState,
    strictness
  );
  const unverified: Unverified[] = [
    ...reconciled.unverified,
    ...qualifyUnifiedState(kase.expect, strictness, evidence),
    ...qualifyUnobserved(kase.expect, strictness, evidence.unobservedChannels),
  ];
  const mismatches: Mismatch[] = [
    ...reconciled.mismatches,
    ...(unobserved.has("outcome") ? [] : judgeOutcome(kase, recorded)),
    ...(unobserved.has("sharedState") ? [] : judgeState(
      "sharedState",
      kase.expect.sharedState,
      recorded.sharedState,
      strictness.sharedState,
      reconciled.handled.sharedState
    )),
    ...(unobserved.has("transientState") ? [] : judgeState(
      "transientState",
      kase.expect.transientState,
      recorded.transientState,
      strictness.transientState,
      reconciled.handled.transientState
    )),
    ...(unobserved.has("secureState") ? [] : judgeState(
      "secureState",
      kase.expect.secureState,
      recorded.secureState,
      strictness.secureState,
      reconciled.handled.secureState
    )),
    ...(unobserved.has("sessionProperties") ? [] : judgeState(
      "sessionProperties",
      kase.expect.sessionProperties,
      recorded.sessionProperties,
      strictness.sessionProperties
    )),
    ...(unobserved.has("callbacks")
      ? []
      : judgeCallbacks(kase.expect, recorded, strictness.callbacks)),
    ...(unobserved.has("openidm")
      ? []
      : judgeOpenidm(kase.expect, recorded, strictness)),
    ...(unobserved.has("http")
      ? []
      : judgeHttp(kase.expect, recorded, strictness.http)),
    ...(unobserved.has("logs")
      ? []
      : judgeLogs(kase.expect, recorded, strictness.logs)),
    ...(unobserved.has("identityWrites")
      ? []
      : judgeIdentityWrites(kase.expect, recorded, strictness.identityWrites)),
    ...(kase.context === "OAUTH2_ACCESS_TOKEN_MODIFICATION"
      ? judgeAccessToken(kase.expect, recorded, strictness.accessToken)
      : []),
    ...(kase.context === "OIDC_CLAIMS"
      ? judgeOidcClaims(kase.expect, recorded, strictness.oidc)
      : []),
    ...(kase.context === "SOCIAL_IDP_PROFILE_TRANSFORMATION"
      ? judgeSocialIdp(kase.expect, recorded, strictness.socialIdp)
      : []),
    ...(kase.context === "CONFIG_PROVIDER_NODE"
      ? judgeConfigProvider(kase.expect, recorded, strictness.configProvider)
      : []),
    ...(kase.context === "OAUTH2_VALIDATE_SCOPE"
      ? judgeValidateScope(kase.expect, recorded, strictness.validateScope)
      : []),
  ];
  return {
    pass: mismatches.length === 0,
    conclusive: unverified.length === 0,
    portable: isPortable(kase),
    mismatches,
    unverified,
    summary: mismatches.length === 0 ? "" : formatSummary(kase, mismatches),
  };
}

interface StateDeclaration {
  channel: StateChannel;
  key: string;
  operation: StateMutation["operation"];
  after?: ExpectedValue;
}

type HandledState = { [K in StateChannel]: Set<string> };

function reconcileUnbucketedState(
  expect: Expect,
  mutations: UnbucketedStateMutation[],
  strictness: Strictness
): {
  mismatches: Mismatch[];
  unverified: Unverified[];
  handled: HandledState;
} {
  const mismatches: Mismatch[] = [];
  const unverified: Unverified[] = [];
  const handled: HandledState = {
    sharedState: new Set(),
    transientState: new Set(),
    secureState: new Set(),
  };
  for (const mutation of mutations) {
    const declarations = stateDeclarations(expect, mutation.key);
    if (declarations.length === 0) {
      const closed = mutation.possibleBuckets.filter(
        (bucket) => !strictness[bucket]
      );
      if (closed.length > 0) {
        mismatches.push(
          miss(
            "nodeState",
            mutation.key,
            "(none)",
            formatMutation(mutation),
            `nodeState: undeclared ${formatMutation(mutation)}; bucket is unobservable (could be ${mutation.possibleBuckets.join(", ")})` +
              `; declare this key in one of expect.${mutation.possibleBuckets.join("/")}, or set ${closed.map((bucket) => `allowUndeclared.${bucket}: true`).join(" and ")}`
          )
        );
      }
      continue;
    }
    for (const declaration of declarations) {
      handled[declaration.channel].add(declaration.key);
    }
    if (declarations.length !== 1) {
      mismatches.push(
        miss(
          "nodeState",
          mutation.key,
          declarations.map(formatDeclaration).join(" or "),
          formatMutation(mutation),
          `nodeState: ${JSON.stringify(mutation.key)} has multiple bucket expectations, but AIC exposes one unified value`
        )
      );
      continue;
    }
    const declaration = declarations[0];
    if (declaration === undefined) {
      continue;
    }
    if (!mutation.possibleBuckets.includes(declaration.channel)) {
      mismatches.push(
        miss(
          declaration.channel,
          mutation.key,
          formatDeclaration(declaration),
          `${formatMutation(mutation)} in ${mutation.possibleBuckets.join(" or ")}`,
          `${declaration.channel}: ${JSON.stringify(mutation.key)} cannot be attributed to that bucket`
        )
      );
      continue;
    }
    if (!mutationMatchesDeclaration(mutation, declaration)) {
      mismatches.push(
        miss(
          declaration.channel,
          mutation.key,
          formatDeclaration(declaration),
          formatMutation(mutation),
          `${declaration.channel}: ${JSON.stringify(mutation.key)} differed in unified nodeState`
        )
      );
      continue;
    }
    unverified.push({
      channel: declaration.channel,
      path: mutation.key,
      message: `${declaration.channel}: ${JSON.stringify(mutation.key)} ${mutation.operation} value matched, but its bucket is unobservable (could be ${mutation.possibleBuckets.join(", ")})`,
    });
  }
  return { mismatches, unverified, handled };
}

function stateDeclarations(expect: Expect, key: string): StateDeclaration[] {
  const declarations: StateDeclaration[] = [];
  for (const channel of STATE_CHANNELS) {
    const state = expect[channel];
    if (Object.prototype.hasOwnProperty.call(state?.added ?? {}, key)) {
      const after = state?.added?.[key];
      if (after !== undefined) {
        declarations.push({ channel, key, operation: "added", after });
      }
    }
    if (Object.prototype.hasOwnProperty.call(state?.changed ?? {}, key)) {
      const after = state?.changed?.[key];
      if (after !== undefined) {
        declarations.push({ channel, key, operation: "changed", after });
      }
    }
    if ((state?.removed ?? []).includes(key)) {
      declarations.push({ channel, key, operation: "removed" });
    }
  }
  return declarations;
}

function mutationMatchesDeclaration(
  mutation: StateMutation,
  declaration: StateDeclaration
): boolean {
  if (mutation.operation !== declaration.operation) {
    return false;
  }
  if (mutation.operation === "removed") {
    return true;
  }
  return declaration.after !== undefined && matchesValue(declaration.after, mutation.after);
}

function formatMutation(mutation: StateMutation): string {
  if (mutation.operation === "removed") {
    return `removed ${formatValue(mutation.key)}`;
  }
  if (mutation.operation === "added") {
    return `added ${formatValue(mutation.key)}=${formatValue(mutation.after)}`;
  }
  return `changed ${formatValue(mutation.key)} from ${formatValue(mutation.before)} to ${formatValue(mutation.after)}`;
}

function formatDeclaration(declaration: StateDeclaration): string {
  if (declaration.operation === "removed") {
    return `${declaration.channel} removed ${formatValue(declaration.key)}`;
  }
  return `${declaration.channel} ${declaration.operation} ${formatValue(declaration.key)}=${formatValue(declaration.after)}`;
}

function qualifyUnobserved(
  expect: Expect,
  strictness: Strictness,
  channels: Channel[]
): Unverified[] {
  const gaps: Unverified[] = [];
  for (const channel of channels) {
    if (!needsObservation(channel, expect, strictness)) {
      continue;
    }
    gaps.push({
      channel,
      path: channel,
      message: `${channel}: the runner cannot observe this channel, so its expectations or undeclared-effect policy were not verified`,
    });
  }
  return gaps;
}

function qualifyUnifiedState(
  expect: Expect,
  strictness: Strictness,
  evidence: RecordingEvidence
): Unverified[] {
  if (evidence.stateBuckets === "exact") {
    return [];
  }
  const needsBuckets =
    STATE_CHANNELS.some((channel) => stateDiffHasEntries(expect[channel])) ||
    STATE_CHANNELS.some((channel) => !strictness[channel]);
  if (!needsBuckets) {
    return [];
  }
  return [
    {
      channel: "nodeState",
      path: "buckets",
      message:
        "nodeState: the runner observes one unified view; per-bucket absence and hidden lower-precedence writes were not verified",
    },
  ];
}

function needsObservation(
  channel: Channel,
  expect: Expect,
  strictness: Strictness
): boolean {
  if (channel === "outcome") {
    return true;
  }
  if (channel === "sharedState" || channel === "transientState" || channel === "secureState" || channel === "sessionProperties") {
    return stateDiffHasEntries(expect[channel]) || !strictness[channel];
  }
  if (channel === "callbacks") {
    return (expect.callbacks?.length ?? 0) > 0 || !strictness.callbacks;
  }
  if (channel === "openidm") {
    return (
      (expect.openidm?.length ?? 0) > 0 ||
      !strictness.openidmWrites ||
      !strictness.openidmReads
    );
  }
  if (channel === "http") {
    return (expect.http?.length ?? 0) > 0 || !strictness.http;
  }
  if (channel === "identityWrites") {
    return (expect.identityWrites?.length ?? 0) > 0 || !strictness.identityWrites;
  }
  return (expect.logs?.length ?? 0) > 0 || !strictness.logs;
}

/**
 * The remedy every undeclared-effect failure ends with. `declared` is true
 * when the failure exists only because the expectation named the channel —
 * reads and log lines are fail-open until then — which is the surprise the
 * hint has to explain.
 */
function remedy(
  flag: AllowUndeclaredChannel,
  key: keyof Expect,
  noun: string,
  declared: boolean
): string {
  return declared
    ? `; declaring expect.${key} makes it exhaustive — declare this ${noun}, or set allowUndeclared.${flag}: true`
    : `; declare this ${noun} in expect.${key}, or set allowUndeclared.${flag}: true`;
}

function stateDiffHasEntries(diff: StateDiff | undefined): boolean {
  return (
    Object.keys(diff?.added ?? {}).length > 0 ||
    Object.keys(diff?.changed ?? {}).length > 0 ||
    (diff?.removed?.length ?? 0) > 0
  );
}

function exactEvidence(): RecordingEvidence {
  return {
    stateBuckets: "exact",
    ambientState: {},
    unbucketedState: [],
    unobservedChannels: [],
  };
}

function resolveStrictness(expect: Expect): Strictness {
  const resolved: Strictness = { ...DEFAULT_ALLOW_UNDECLARED };
  if (expect.openidm !== undefined) {
    resolved.openidmReads = false;
  }
  if (expect.logs !== undefined) {
    resolved.logs = false;
  }
  const flags: AllowUndeclared | undefined = expect.allowUndeclared;
  if (flags === undefined) {
    return resolved;
  }
  for (const key of ALLOW_UNDECLARED_CHANNELS) {
    const value = flags[key];
    if (value !== undefined) {
      resolved[key] = value;
    }
  }
  return resolved;
}

function judgeOutcome(kase: Case, effects: CompleteRecordedEffects): Mismatch[] {
  if (effects.outcome === kase.expect.outcome) {
    return [];
  }
  const actual =
    effects.outcome === null ? "<no outcome>" : formatValue(effects.outcome);
  const wanted =
    kase.expect.outcome === null
      ? "<no outcome>"
      : formatValue(kase.expect.outcome);
  // Expecting no outcome is expecting a suspend, so say that rather than
  // "expected <no outcome>, actual \"true\"" — the script did not fail to
  // produce something, it failed to stop and ask.
  if (kase.expect.outcome === null) {
    return [
      miss(
        "outcome",
        "outcome",
        wanted,
        actual,
        `outcome: expected the pass to suspend with callbacks and decide nothing, but it reached ${actual}`
      ),
    ];
  }
  // An outcome outside the declared vocabulary is a harness configuration
  // fault, not the script behaving differently from expectation, and it
  // supersedes the ordinary mismatch because the ordinary message ("expected
  // X, actual Y") invites you to go and look at Y's branch — when the real
  // problem is that Y is not wired into the journey at all. On a tenant this
  // is the difference between a readable failure and a bare 401.
  if (
    kase.outcomes !== undefined &&
    effects.outcome !== null &&
    !kase.outcomes.includes(effects.outcome)
  ) {
    const declared = kase.outcomes.map((name) => formatValue(name)).join(", ");
    return [
      miss(
        "outcome",
        "outcome",
        wanted,
        actual,
        `outcome: script produced ${actual}, which case.outcomes does not declare [${declared}] — fix the typo or declare it; on a tenant an undeclared outcome answers 401 with no callback, identical to a compile error`
      ),
    ];
  }
  const message =
    effects.outcome === null
      ? effects.callbacks.length > 0
        ? `outcome: expected ${wanted}, but the script queued ${effects.callbacks.length} callbacks, so AM suspends${effects.discardedOutcome === null || effects.discardedOutcome === undefined ? "" : ` and discards the recorded outcome ${formatValue(effects.discardedOutcome)}`}`
        : `outcome: expected ${wanted}, script produced no outcome`
      : `outcome: expected ${wanted}, actual ${actual}`;
  return [miss("outcome", "outcome", wanted, actual, message)];
}

/**
 * The token the access-token context leaves behind, fail-closed like the state
 * channels: every difference from the starting token must be declared, unless
 * `allowUndeclared.accessToken`. A run that recorded no token is a mismatch,
 * never a pass.
 */
function judgeAccessToken(
  expect: Expect,
  recorded: CompleteRecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const effect = recorded.accessToken;
  if (effect === undefined) {
    return [
      miss("accessToken", "accessToken", "a recorded token", "<none>",
        "accessToken: the runner recorded no token for an access-token modification case"),
    ];
  }
  const want = expect.accessToken ?? {};
  const mismatches: Mismatch[] = [];
  const prefixed = (prefix: string, items: Mismatch[]): Mismatch[] =>
    items.map((item) => ({
      ...item,
      path: `${prefix}.${item.path}`,
      message: item.message.replace(/^accessToken: /, `accessToken.${prefix}: `),
    }));
  mismatches.push(
    ...prefixed("fields", judgeState("accessToken", want.fields,
      { initial: effect.initial.fields, final: effect.final.fields }, allowUndeclared)),
    ...prefixed("responseFields", judgeState("accessToken", want.responseFields,
      { initial: effect.initial.responseFields, final: effect.final.responseFields }, allowUndeclared))
  );
  const issued = effect.refusal === null;
  const wantIssued = want.issued ?? true;
  if (issued !== wantIssued) {
    mismatches.push(
      miss("accessToken", "issued", String(wantIssued), String(issued),
        issued
          ? "accessToken.issued: expected AM to refuse the token, but nothing the script did makes it refuse"
          : `accessToken.issued: AM would refuse the token request after this script (500 server_error: ${effect.refusal})`)
    );
  }
  // Both directions: validation refuses a repeated scope in a case, and the
  // harvested scope is a set, but the comparison must not rely on either.
  const sameSet = (a: string[], b: string[]): boolean => {
    const left = new Set(a);
    const right = new Set(b);
    return left.size === right.size && [...left].every((item) => right.has(item));
  };
  if (want.scope !== undefined) {
    if (!sameSet(want.scope, effect.final.scope)) {
      mismatches.push(
        miss("accessToken", "scope", formatValue(want.scope), formatValue(effect.final.scope),
          "accessToken.scope: the final scope differed (compared as a set)")
      );
    }
  } else if (!allowUndeclared && issued && !sameSet(effect.initial.scope, effect.final.scope)) {
    mismatches.push(
      miss("accessToken", "scope", "unchanged", formatValue(effect.final.scope),
        "accessToken.scope: undeclared change; declare expect.accessToken.scope or set allowUndeclared.accessToken")
    );
  }
  for (const key of TOKEN_VALUE_KEYS) {
    const before = effect.initial[key];
    const after = effect.final[key];
    if (key in want) {
      const expected = want[key];
      if (expected !== undefined && !matchesValue(expected, after)) {
        mismatches.push(
          miss("accessToken", key, formatValue(expected), formatValue(after), `accessToken.${key}: differed`)
        );
      }
      continue;
    }
    if (!allowUndeclared && !deepEqual(before, after)) {
      mismatches.push(
        miss("accessToken", key, `unchanged ${formatValue(before)}`, formatValue(after),
          `accessToken.${key}: undeclared change; declare expect.accessToken.${key} or set allowUndeclared.accessToken`)
      );
    }
  }
  return mismatches;
}

/**
 * The claims an OIDC claims script returns, fail-closed: returned values and
 * composite scopes must be declared unless `allowUndeclared.oidc`. The id
 * token's script claims are a function of the returned values and the seeded
 * subject, so once the userinfo values are judged an undeclared id token adds
 * nothing to check; a declared one is still compared exactly. A run that
 * recorded no result is a mismatch, never a pass.
 */
function judgeOidcClaims(
  expect: Expect,
  recorded: CompleteRecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const effect = recorded.oidc;
  if (effect === undefined) {
    return [
      miss("oidc", "oidc", "a recorded result", "<none>",
        "oidc: the runner recorded no result for an OIDC claims case"),
    ];
  }
  const want = expect.oidc ?? {};
  const mismatches: Mismatch[] = [];
  const issued = effect.refusal === null;
  const wantIssued = want.issued ?? true;
  if (issued !== wantIssued) {
    mismatches.push(
      miss("oidc", "issued", String(wantIssued), String(issued),
        issued
          ? "oidc.issued: expected AM to fail the token request, but nothing the script did makes it fail"
          : `oidc.issued: AM would fail the token request after this script (500 server_error: ${effect.refusal})`)
    );
  }
  const values: { key: "userinfo" | "idToken" | "compositeScopes"; actual: JsonValue; undeclared: boolean }[] = [
    { key: "userinfo", actual: effect.userinfo, undeclared: Object.keys(effect.userinfo).length > 0 },
    { key: "idToken", actual: effect.idToken, undeclared: false },
    {
      key: "compositeScopes",
      actual: effect.compositeScopes,
      undeclared: !isEmptyComposite(effect.compositeScopes),
    },
  ];
  for (const { key, actual, undeclared } of values) {
    if (key in want) {
      const expected = want[key];
      if (expected !== undefined && !matchesValue(expected, actual)) {
        mismatches.push(miss("oidc", key, formatValue(expected), formatValue(actual), `oidc.${key}: differed`));
      }
      continue;
    }
    if (!allowUndeclared && issued && undeclared) {
      mismatches.push(
        miss("oidc", key, "none", formatValue(actual),
          `oidc.${key}: undeclared claims; declare expect.oidc.${key} or set allowUndeclared.oidc`)
      );
    }
  }
  return mismatches;
}

/**
 * The profile a social IdP transformation returns, fail-closed: a returned
 * profile must be declared unless `allowUndeclared.socialIdp`. A run that
 * recorded no result is a mismatch, never a pass.
 */
function judgeSocialIdp(
  expect: Expect,
  recorded: CompleteRecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const effect = recorded.socialIdp;
  if (effect === undefined) {
    return [
      miss("socialIdp", "socialIdp", "a recorded result", "<none>",
        "socialIdp: the runner recorded no result for a social IdP profile transformation case"),
    ];
  }
  const want = expect.socialIdp ?? {};
  const mismatches: Mismatch[] = [];
  const accepted = effect.refusal === null;
  const wantAccepted = want.accepted ?? true;
  if (accepted !== wantAccepted) {
    mismatches.push(
      miss("socialIdp", "accepted", String(wantAccepted), String(accepted),
        accepted
          ? "socialIdp.accepted: expected AM to fail the journey, but the script returned a profile"
          : `socialIdp.accepted: ${effect.refusal ?? ""}`)
    );
  }
  if ("profile" in want) {
    const expected = want.profile;
    if (expected !== undefined && !matchesValue(expected, effect.profile)) {
      mismatches.push(
        miss("socialIdp", "profile", formatValue(expected), formatValue(effect.profile), "socialIdp.profile: differed")
      );
    }
  } else if (!allowUndeclared && accepted) {
    mismatches.push(
      miss("socialIdp", "profile", "none", formatValue(effect.profile),
        "socialIdp.profile: undeclared profile; declare expect.socialIdp.profile or set allowUndeclared.socialIdp")
    );
  }
  return mismatches;
}

/**
 * The `config` map a config provider node script builds, fail-closed like the
 * social profile: a map must be declared unless `allowUndeclared.configProvider`,
 * and a run that recorded no result is a mismatch.
 */
function judgeConfigProvider(
  expect: Expect,
  recorded: CompleteRecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const effect = recorded.configProvider;
  if (effect === undefined) {
    return [
      miss("configProvider", "configProvider", "a recorded result", "<none>",
        "configProvider: the runner recorded no result for a config provider node case"),
    ];
  }
  const want = expect.configProvider ?? {};
  const mismatches: Mismatch[] = [];
  const accepted = effect.refusal === null;
  const wantAccepted = want.accepted ?? true;
  if (accepted !== wantAccepted) {
    mismatches.push(
      miss("configProvider", "accepted", String(wantAccepted), String(accepted),
        accepted
          ? "configProvider.accepted: expected AM to fail the journey, but the script set a config map"
          : `configProvider.accepted: ${effect.refusal ?? ""}`)
    );
  }
  if ("config" in want) {
    const expected = want.config;
    if (expected !== undefined && !matchesValue(expected, effect.config)) {
      mismatches.push(
        miss("configProvider", "config", formatValue(expected), formatValue(effect.config), "configProvider.config: differed")
      );
    }
  } else if (!allowUndeclared && accepted) {
    mismatches.push(
      miss("configProvider", "config", "none", formatValue(effect.config),
        "configProvider.config: undeclared config map; declare expect.configProvider.config or set allowUndeclared.configProvider")
    );
  }
  return mismatches;
}

/**
 * The token request a validate scope script decides, fail-closed: granted
 * scopes must be declared unless `allowUndeclared.validateScope`, and a run
 * that recorded no result is a mismatch. Scopes compare as a set, as AM
 * grants one.
 */
function judgeValidateScope(expect: Expect, recorded: CompleteRecordedEffects, allowUndeclared: boolean): Mismatch[] {
  const effect = recorded.validateScope;
  if (effect === undefined) {
    return [
      miss("validateScope", "validateScope", "a recorded result", "<none>",
        "validateScope: the runner recorded no result for a validate scope case"),
    ];
  }
  const want = expect.validateScope ?? {};
  const mismatches: Mismatch[] = [];
  const accepted = effect.refusal === null;
  const wantAccepted = want.accepted ?? want.invalidScope === undefined;
  if (accepted !== wantAccepted) {
    mismatches.push(
      miss("validateScope", "accepted", String(wantAccepted), String(accepted),
        accepted
          ? `validateScope.accepted: expected AM to issue no token, but it granted ${formatValue(effect.granted)}`
          : `validateScope.accepted: AM answers ${effect.status}: ${effect.refusal ?? ""}`)
    );
  }
  if (want.invalidScope !== undefined && want.invalidScope !== effect.invalidScope) {
    mismatches.push(
      miss("validateScope", "invalidScope", JSON.stringify(want.invalidScope), formatValue(effect.invalidScope),
        "validateScope.invalidScope: differed")
    );
  }
  if (want.granted !== undefined) {
    const got = effect.granted ?? [];
    const same = got.length === new Set(want.granted).size && got.every((s) => want.granted!.includes(s));
    if (!same || effect.granted === null) {
      mismatches.push(
        miss("validateScope", "granted", formatValue(want.granted), formatValue(effect.granted),
          "validateScope.granted: differed (compared as a set)")
      );
    }
  } else if (!allowUndeclared && accepted) {
    mismatches.push(
      miss("validateScope", "granted", "none", formatValue(effect.granted),
        "validateScope.granted: undeclared grant; declare expect.validateScope.granted or set allowUndeclared.validateScope")
    );
  }
  return mismatches;
}

// No composite scopes, whether the script passed null or an empty map, adds
// nothing a case author needs to declare.
function isEmptyComposite(value: JsonValue): boolean {
  return value === null || (typeof value === "object" && !Array.isArray(value) && Object.keys(value).length === 0);
}

function diffState(
  initial: JsonObject,
  final: JsonObject
): { added: JsonObject; changed: JsonObject; removed: string[] } {
  const added: JsonObject = {};
  const changed: JsonObject = {};
  const removed: string[] = [];
  const initialKeys = new Set(Object.keys(initial));
  const finalKeys = new Set(Object.keys(final));
  for (const key of finalKeys) {
    if (!initialKeys.has(key)) {
      const value = final[key];
      if (value !== undefined) {
        defineOwn(added, key, value);
      }
      continue;
    }
    if (!deepEqual(initial[key], final[key])) {
      const value = final[key];
      if (value !== undefined) {
        defineOwn(changed, key, value);
      }
    }
  }
  for (const key of initialKeys) {
    if (!finalKeys.has(key)) {
      removed.push(key);
    }
  }
  removed.sort();
  return { added, changed, removed };
}

function judgeState(
  channel: "sharedState" | "transientState" | "secureState" | "sessionProperties" | "accessToken",
  expected: StateDiff | undefined,
  bucket: StateBucket,
  allowUndeclared: boolean,
  handled: ReadonlySet<string> = new Set()
): Mismatch[] {
  const actual = diffState(bucket.initial, bucket.final);
  const expectedAdded = expected?.added ?? {};
  const expectedChanged = expected?.changed ?? {};
  const expectedRemoved = expected?.removed ?? [];
  const expectedRemovedSet = new Set(expectedRemoved);
  const mismatches: Mismatch[] = [];

  for (const key of Object.keys(expectedAdded)) {
    if (handled.has(key)) {
      continue;
    }
    const want = expectedAdded[key];
    if (Object.prototype.hasOwnProperty.call(actual.added, key)) {
      if (want !== undefined && !matchesValue(want, actual.added[key])) {
        mismatches.push(
          miss(
            channel,
            key,
            formatValue(want),
            formatValue(actual.added[key]),
            `${channel}: added ${formatValue(key)} differed`
          )
        );
      }
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(actual.changed, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `added ${formatValue(want)}`,
          `changed to ${formatValue(actual.changed[key])}`,
          `${channel}: ${formatValue(key)} was changed, not added (already present in initial state)`
        )
      );
      continue;
    }
    if (actual.removed.includes(key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `added ${formatValue(want)}`,
          "removed",
          `${channel}: ${formatValue(key)} was removed, not added`
        )
      );
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(bucket.final, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `added ${formatValue(want)}`,
          `unchanged ${formatValue(bucket.final[key])}`,
          `${channel}: ${formatValue(key)} was unchanged, not added`
        )
      );
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        formatValue(want),
        "<absent>",
        `${channel}: missing added ${formatValue(key)}`
      )
    );
  }

  for (const key of Object.keys(expectedChanged)) {
    if (handled.has(key)) {
      continue;
    }
    const want = expectedChanged[key];
    if (Object.prototype.hasOwnProperty.call(actual.changed, key)) {
      if (want !== undefined && !matchesValue(want, actual.changed[key])) {
        mismatches.push(
          miss(
            channel,
            key,
            formatValue(want),
            formatValue(actual.changed[key]),
            `${channel}: changed ${formatValue(key)} differed`
          )
        );
      }
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(actual.added, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `changed to ${formatValue(want)}`,
          `added ${formatValue(actual.added[key])}`,
          `${channel}: ${formatValue(key)} was added, not changed (absent from initial state)`
        )
      );
      continue;
    }
    if (actual.removed.includes(key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `changed to ${formatValue(want)}`,
          "removed",
          `${channel}: ${formatValue(key)} was removed, not changed`
        )
      );
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(bucket.final, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          `changed to ${formatValue(want)}`,
          `unchanged ${formatValue(bucket.final[key])}`,
          `${channel}: ${formatValue(key)} did not change`
        )
      );
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        formatValue(want),
        "<absent>",
        `${channel}: missing changed ${formatValue(key)}`
      )
    );
  }

  for (const key of expectedRemoved) {
    if (handled.has(key)) {
      continue;
    }
    if (actual.removed.includes(key)) {
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(bucket.final, key)) {
      mismatches.push(
        miss(
          channel,
          key,
          "removed",
          formatValue(bucket.final[key]),
          `${channel}: ${formatValue(key)} was not removed`
        )
      );
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        "removed",
        "<absent from initial state>",
        `${channel}: ${formatValue(key)} was absent, not removed`
      )
    );
  }

  if (allowUndeclared) {
    return mismatches;
  }

  const mentioned = (key: string): boolean =>
    Object.prototype.hasOwnProperty.call(expectedAdded, key) ||
    Object.prototype.hasOwnProperty.call(expectedChanged, key) ||
    expectedRemovedSet.has(key);

  for (const key of Object.keys(actual.added)) {
    if (mentioned(key)) {
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        "(none)",
        formatValue(actual.added[key]),
        `${channel}: undeclared added ${formatValue(key)}${remedy(channel, channel, "key", false)}`
      )
    );
  }
  for (const key of Object.keys(actual.changed)) {
    if (mentioned(key)) {
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        "(none)",
        formatValue(actual.changed[key]),
        `${channel}: undeclared changed ${formatValue(key)}${remedy(channel, channel, "key", false)}`
      )
    );
  }
  for (const key of actual.removed) {
    if (mentioned(key)) {
      continue;
    }
    mismatches.push(
      miss(
        channel,
        key,
        "(none)",
        "removed",
        `${channel}: undeclared removed ${formatValue(key)}${remedy(channel, channel, "key", false)}`
      )
    );
  }
  return mismatches;
}

function judgeCallbacks(
  expect: Expect,
  effects: CompleteRecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const expected = expect.callbacks ?? [];
  const actual = effects.callbacks;
  if (allowUndeclared) {
    if (isSubsequence(expected, actual)) {
      return [];
    }
    return [
      miss(
        "callbacks",
        "callbacks",
        formatValue(expected),
        formatValue(actual),
        "callbacks: expected callbacks did not appear in order"
      ),
    ];
  }
  const mismatches: Mismatch[] = [];
  const length = Math.max(expected.length, actual.length);
  for (let index = 0; index < length; index += 1) {
    const want = expected[index];
    const got = actual[index];
    if (want !== undefined && matchesValue(want, got)) {
      continue;
    }
    mismatches.push(
      miss(
        "callbacks",
        `[${index}]`,
        want === undefined ? "(none)" : formatValue(want),
        got === undefined ? "(none)" : formatValue(got),
        want === undefined
          ? `callbacks: [${index}] differed${remedy("callbacks", "callbacks", "callback", false)}`
          : `callbacks: [${index}] differed`
      )
    );
  }
  return mismatches;
}

function isSubsequence(
  expected: NonNullable<Expect["callbacks"]>,
  actual: CallbackEffect[]
): boolean {
  let index = 0;
  for (const item of actual) {
    const want = expected[index];
    if (want !== undefined && matchesValue(want, item)) {
      index += 1;
    }
  }
  return index === expected.length;
}

function judgeOpenidm(
  expect: Expect,
  effects: CompleteRecordedEffects,
  strictness: Strictness
): Mismatch[] {
  const expected = expect.openidm ?? [];
  const mismatches: Mismatch[] = [];
  const declared = new Set<number>();
  for (const item of expected) {
    const matched = matchIndices(item, effects.openidm, openidmMatches);
    const times = item.times !== undefined ? item.times : 1;
    for (const index of matched) {
      declared.add(index);
    }
    if (matched.length !== times) {
      mismatches.push(
        miss(
          "openidm",
          formatOpenidmExpect(item),
          `${times}`,
          `${matched.length}`,
          `openidm: expected ${times} ${formatOpenidmExpect(item)}, actual ${matched.length}`
        )
      );
    }
  }
  for (let index = 0; index < effects.openidm.length; index += 1) {
    if (declared.has(index)) {
      continue;
    }
    const actual = effects.openidm[index];
    if (actual === undefined) {
      continue;
    }
    const write = WRITE_METHODS.has(actual.method);
    if (write && strictness.openidmWrites) {
      continue;
    }
    if (!write && strictness.openidmReads) {
      continue;
    }
    const kind = write ? "write" : "read";
    mismatches.push(
      miss(
        "openidm",
        formatOpenidm(actual),
        "(none)",
        formatOpenidm(actual),
        `openidm: undeclared ${kind} ${formatOpenidm(actual)}` +
          (write
            ? remedy("openidmWrites", "openidm", "call", false)
            : remedy("openidmReads", "openidm", "call", expect.openidm !== undefined))
      )
    );
  }
  return mismatches;
}

function judgeHttp(
  expect: Expect,
  effects: CompleteRecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const expected = expect.http ?? [];
  const mismatches: Mismatch[] = [];
  const declared = new Set<number>();
  for (const item of expected) {
    const matched = matchIndices(item, effects.http, httpMatches);
    const times = item.times !== undefined ? item.times : 1;
    for (const index of matched) {
      declared.add(index);
    }
    if (matched.length !== times) {
      mismatches.push(
        miss(
          "http",
          formatHttpExpect(item),
          `${times}`,
          `${matched.length}`,
          `http: expected ${times} request matching ${formatHttpExpect(item)}, actual ${matched.length}`
        )
      );
    }
  }
  if (allowUndeclared) {
    return mismatches;
  }
  for (let index = 0; index < effects.http.length; index += 1) {
    if (declared.has(index)) {
      continue;
    }
    const actual = effects.http[index];
    if (actual === undefined) {
      continue;
    }
    mismatches.push(
      miss(
        "http",
        formatHttp(actual),
        "(none)",
        formatHttp(actual),
        `http: undeclared request ${formatHttp(actual)}${remedy("http", "http", "request", false)}`
      )
    );
  }
  return mismatches;
}

function judgeLogs(
  expect: Expect,
  effects: CompleteRecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const expected = expect.logs ?? [];
  const mismatches: Mismatch[] = [];
  const declared = new Set<number>();
  for (const item of expected) {
    const matched = matchIndices(item, effects.logs, logMatches);
    const times = item.times !== undefined ? item.times : 1;
    for (const index of matched) {
      declared.add(index);
    }
    if (matched.length !== times) {
      mismatches.push(
        miss(
          "logs",
          formatLogExpect(item),
          `${times}`,
          `${matched.length}`,
          `logs: expected ${times} line matching ${formatLogExpect(item)}, actual ${matched.length}`
        )
      );
    }
  }
  if (allowUndeclared) {
    return mismatches;
  }
  for (let index = 0; index < effects.logs.length; index += 1) {
    if (declared.has(index)) {
      continue;
    }
    const actual = effects.logs[index];
    if (actual === undefined) {
      continue;
    }
    mismatches.push(
      miss(
        "logs",
        formatLog(actual),
        "(none)",
        formatLog(actual),
        `logs: undeclared line ${formatLog(actual)}${remedy("logs", "logs", "line", expect.logs !== undefined)}`
      )
    );
  }
  return mismatches;
}

function judgeIdentityWrites(
  expect: Expect,
  effects: CompleteRecordedEffects,
  allowUndeclared: boolean
): Mismatch[] {
  const expected = expect.identityWrites ?? [];
  const mismatches: Mismatch[] = [];
  const declared = new Set<number>();
  for (const item of expected) {
    const matched = matchIndices(item, effects.identityWrites, identityWriteMatches);
    const times = item.times !== undefined ? item.times : 1;
    for (const index of matched) {
      declared.add(index);
    }
    if (matched.length !== times) {
      mismatches.push(
        miss(
          "identityWrites",
          formatIdentityWriteExpect(item),
          `${times}`,
          `${matched.length}`,
          `identityWrites: expected ${times} write matching ${formatIdentityWriteExpect(item)}, actual ${matched.length}`
        )
      );
    }
  }
  if (allowUndeclared) {
    return mismatches;
  }
  effects.identityWrites.forEach((actual, index) => {
    if (declared.has(index)) {
      return;
    }
    mismatches.push(
      miss(
        "identityWrites",
        formatIdentityWrite(actual),
        "(none)",
        formatIdentityWrite(actual),
        `identityWrites: undeclared write ${formatIdentityWrite(actual)}${remedy("identityWrites", "identityWrites", "write", false)}`
      )
    );
  });
  return mismatches;
}

export function identityWriteMatches(expected: IdentityWriteExpect, actual: IdentityWriteEffect): boolean {
  return matchesPattern(expected.identity, actual.identity) &&
    matchesPattern(expected.attribute, actual.attribute) &&
    (expected.values === undefined || matchesValue(expected.values, actual.values));
}

function formatIdentityWrite(op: IdentityWriteEffect): string {
  return `${op.identity} ${op.attribute}=${formatValue(op.values)}`;
}

function formatIdentityWriteExpect(item: IdentityWriteExpect): string {
  const values = item.values === undefined ? "" : `=${formatValue(item.values)}`;
  return `${String(item.identity)} ${String(item.attribute)}${values}`;
}

function matchIndices<E, A>(
  expected: E,
  actuals: A[],
  matches: (expected: E, actual: A) => boolean
): number[] {
  const indices: number[] = [];
  for (let index = 0; index < actuals.length; index += 1) {
    const actual = actuals[index];
    if (actual !== undefined && matches(expected, actual)) {
      indices.push(index);
    }
  }
  return indices;
}

/** Also used by the lane diff to identify effects actually declared by a body matcher. */
export function openidmMatches(expected: OpenidmExpect, actual: OpenidmEffect): boolean {
  if (actual.method !== expected.method) {
    return false;
  }
  if (!matchesPattern(expected.resource, actual.resource)) {
    return false;
  }
  if (expected.body !== undefined &&
    (!Object.prototype.hasOwnProperty.call(actual, "body") ||
      !matchesValue(expected.body, actual.body))) {
    return false;
  }
  if (expected.actionName !== undefined) {
    if (actual.actionName === undefined) {
      return false;
    }
    if (!matchesPattern(expected.actionName, actual.actionName)) {
      return false;
    }
  }
  return true;
}

/** Also used by the lane diff to identify effects actually declared by a body matcher. */
export function httpMatches(expected: HttpExpect, actual: HttpEffect): boolean {
  if (!matchesPattern(expected.url, actual.url)) {
    return false;
  }
  if (expected.method !== undefined && actual.method !== expected.method) {
    return false;
  }
  if (expected.body !== undefined &&
    (!Object.prototype.hasOwnProperty.call(actual, "body") ||
      !matchesValue(expected.body, actual.body))) {
    return false;
  }
  return true;
}

function logMatches(expected: LogExpect, actual: LogEffect): boolean {
  if (expected.level !== undefined && actual.level !== expected.level) {
    return false;
  }
  return matchesPattern(expected.message, actual.message);
}

/**
 * Validate effects from any producer and fill the channels it omitted. The
 * omitted ones are listed in `evidence.unobservedChannels`, so a judge or a
 * diff reports them as gaps instead of reading them as empty.
 */
export function normaliseEffects(raw: unknown): CompleteRecordedEffects {
  return parseEffects(raw);
}

function parseEffects(raw: unknown): CompleteRecordedEffects {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects is not an object");
  }
  const absent: Channel[] = [];
  for (const key of CHANNELS) {
    // An own key holding `undefined` is an absent channel, not an empty one:
    // `{identityWrites: undefined}` must not read as "observed, no writes".
    if (raw[key] !== undefined) {
      continue;
    }
    if (!LATER_CHANNELS.has(key)) {
      throw new Error(
        `rhino-local: effects is missing ${key} — a runner that does not record a channel would silently assert nothing`
      );
    }
    absent.push(key);
  }
  if (raw.outcome !== null && typeof raw.outcome !== "string") {
    throw new Error("rhino-local: effects.outcome must be a string or null");
  }
  const recorded: CompleteRecordedEffects = {
    outcome: raw.outcome,
    sharedState: parseStateBucket(raw.sharedState, "effects.sharedState"),
    transientState: parseStateBucket(
      raw.transientState,
      "effects.transientState"
    ),
    secureState: parseStateBucket(raw.secureState, "effects.secureState"),
    sessionProperties: raw.sessionProperties === undefined
      ? { initial: {}, final: {} }
      : parseStateBucket(raw.sessionProperties, "effects.sessionProperties"),
    callbacks: parseArray(raw.callbacks, "effects.callbacks", parseCallbackEffect),
    openidm: parseArray(raw.openidm, "effects.openidm", parseOpenidmEffect),
    http: parseArray(raw.http, "effects.http", parseHttpEffect),
    logs: parseArray(raw.logs, "effects.logs", parseLogEffect),
    identityWrites: raw.identityWrites === undefined
      ? []
      : parseArray(raw.identityWrites, "effects.identityWrites", parseIdentityWriteEffect),
  };
  if (raw.accessToken !== undefined) {
    recorded.accessToken = parseAccessTokenEffect(raw.accessToken);
  }
  if (raw.oidc !== undefined) {
    recorded.oidc = parseOidcClaimsEffect(raw.oidc);
  }
  if (raw.socialIdp !== undefined) {
    recorded.socialIdp = parseSocialIdpEffect(raw.socialIdp);
  }
  if (raw.configProvider !== undefined) {
    recorded.configProvider = parseConfigProviderEffect(raw.configProvider);
  }
  if (raw.validateScope !== undefined) {
    recorded.validateScope = parseValidateScopeEffect(raw.validateScope);
  }
  if (raw.discardedOutcome !== undefined) {
    if (raw.discardedOutcome !== null && typeof raw.discardedOutcome !== "string") {
      throw new Error("rhino-local: effects.discardedOutcome must be a string or null");
    }
    recorded.discardedOutcome = raw.discardedOutcome;
  }
  if (raw.managedStore !== undefined) {
    // What carryGiven seeds the next pass from; dropping it would replay the
    // pass's starting records and undo every write the pass made.
    recorded.managedStore = parseManagedStore(raw.managedStore);
  }
  if (raw.identityCustomAttrs !== undefined) {
    recorded.identityCustomAttrs = parseIdentityCustomAttrs(raw.identityCustomAttrs, recorded.managedStore, "effects.identityCustomAttrs");
  }
  if (raw.identityCustomAttrsOwnedKeys !== undefined) {
    recorded.identityCustomAttrsOwnedKeys = parseIdentityCustomAttrsOwnedKeys(raw.identityCustomAttrsOwnedKeys, recorded.managedStore, recorded.identityCustomAttrs, "effects.identityCustomAttrsOwnedKeys");
  }
  if (raw.evidence !== undefined) {
    recorded.evidence = parseRecordingEvidence(raw.evidence);
    for (const channel of recorded.evidence.unobservedChannels) {
      if (recordedChannelHasValues(recorded, channel)) {
        throw new Error(
          `rhino-local: effects.${channel} cannot contain values while evidence marks it unobserved`
        );
      }
    }
  }
  if (absent.length > 0) {
    const evidence = recorded.evidence ?? exactEvidence();
    recorded.evidence = {
      ...evidence,
      unobservedChannels: [...new Set([...evidence.unobservedChannels, ...absent])],
    };
  }
  return recorded;
}

function parseAccessTokenEffect(raw: unknown): AccessTokenEffect {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects.accessToken is not an object");
  }
  if (raw.refusal !== null && typeof raw.refusal !== "string") {
    throw new Error("rhino-local: effects.accessToken.refusal must be a string or null");
  }
  return {
    initial: parseAccessTokenState(raw.initial, "effects.accessToken.initial"),
    final: parseAccessTokenState(raw.final, "effects.accessToken.final"),
    refusal: raw.refusal,
  };
}

function parseSocialIdpEffect(raw: unknown): SocialIdpEffect {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects.socialIdp is not an object");
  }
  if (raw.returned !== "profile" && raw.returned !== "none") {
    throw new Error(`rhino-local: effects.socialIdp.returned must be "profile" or "none"`);
  }
  if (raw.refusal !== null && typeof raw.refusal !== "string") {
    throw new Error("rhino-local: effects.socialIdp.refusal must be a string or null");
  }
  return {
    returned: raw.returned,
    profile: raw.profile === null ? null : parseJsonObject(raw.profile, "effects.socialIdp.profile"),
    refusal: raw.refusal,
  };
}

function parseValidateScopeEffect(raw: unknown): ValidateScopeEffect {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects.validateScope is not an object");
  }
  if (raw.returned !== "scopes" && raw.returned !== "none") {
    throw new Error(`rhino-local: effects.validateScope.returned must be "scopes" or "none"`);
  }
  if (typeof raw.status !== "number" || ![200, 400, 403, 500].includes(raw.status)) {
    throw new Error("rhino-local: effects.validateScope.status must be 200, 400, 403 or 500");
  }
  const strOrNull = (v: unknown, key: string): string | null => {
    if (v !== null && typeof v !== "string") throw new Error(`rhino-local: effects.validateScope.${key} must be a string or null`);
    return v;
  };
  let granted: string[] | null = null;
  if (raw.granted !== null) {
    if (!Array.isArray(raw.granted) || raw.granted.some((s) => typeof s !== "string")) {
      throw new Error("rhino-local: effects.validateScope.granted must be an array of strings or null");
    }
    granted = raw.granted.slice() as string[];
  }
  return {
    returned: raw.returned,
    status: raw.status,
    granted,
    invalidScope: strOrNull(raw.invalidScope, "invalidScope"),
    refusal: strOrNull(raw.refusal, "refusal"),
  };
}

function parseConfigProviderEffect(raw: unknown): ConfigProviderEffect {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects.configProvider is not an object");
  }
  if (raw.returned !== "config" && raw.returned !== "none") {
    throw new Error(`rhino-local: effects.configProvider.returned must be "config" or "none"`);
  }
  if (raw.refusal !== null && typeof raw.refusal !== "string") {
    throw new Error("rhino-local: effects.configProvider.refusal must be a string or null");
  }
  return {
    returned: raw.returned,
    config: raw.config === null ? null : parseJsonObject(raw.config, "effects.configProvider.config"),
    refusal: raw.refusal,
  };
}

function parseOidcClaimsEffect(raw: unknown): OidcClaimsEffect {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects.oidc is not an object");
  }
  if (raw.returned !== "claims" && raw.returned !== "none") {
    throw new Error(`rhino-local: effects.oidc.returned must be "claims" or "none"`);
  }
  if (raw.refusal !== null && typeof raw.refusal !== "string") {
    throw new Error("rhino-local: effects.oidc.refusal must be a string or null");
  }
  return {
    returned: raw.returned,
    userinfo: parseJsonObject(raw.userinfo, "effects.oidc.userinfo"),
    idToken: parseJsonObject(raw.idToken, "effects.oidc.idToken"),
    compositeScopes: parseJsonValue(raw.compositeScopes, "effects.oidc.compositeScopes"),
    refusal: raw.refusal,
  };
}

function parseAccessTokenState(raw: unknown, path: string): AccessTokenState {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (!Array.isArray(raw.scope) || raw.scope.some((item) => typeof item !== "string")) {
    throw new Error(`rhino-local: ${path}.scope must be an array of strings`);
  }
  const scope = (raw.scope as string[]).slice();
  if (typeof raw.expiryTime !== "number") {
    throw new Error(`rhino-local: ${path}.expiryTime must be a number`);
  }
  const value = (key: string): AccessTokenState["nonce"] =>
    raw[key] === undefined ? null : parseJsonValue(raw[key], `${path}.${key}`);
  return {
    fields: parseJsonObject(raw.fields, `${path}.fields`),
    responseFields: parseJsonObject(raw.responseFields, `${path}.responseFields`),
    scope,
    expiryTime: raw.expiryTime,
    nonce: value("nonce"),
    authLevel: value("authLevel"),
    claims: value("claims"),
    act: value("act"),
    mayAct: value("mayAct"),
    permissions: value("permissions"),
  };
}

function parseManagedStore(raw: unknown): Record<string, JsonObject[]> {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects.managedStore is not an object");
  }
  const store: Record<string, JsonObject[]> = {};
  for (const [collection, rows] of Object.entries(raw)) {
    store[collection] = parseArray(
      rows,
      `effects.managedStore.${collection}`,
      (value, path) => parseJsonObject(value, path)
    );
  }
  return store;
}

function parseRecordingEvidence(raw: unknown): RecordingEvidence {
  if (!isPlainObject(raw)) {
    throw new Error("rhino-local: effects.evidence is not an object");
  }
  for (const key of [
    "stateBuckets",
    "ambientState",
    "unbucketedState",
    "unobservedChannels",
  ]) {
    if (!(key in raw)) {
      throw new Error(`rhino-local: effects.evidence is missing ${key}`);
    }
  }
  const unobservedChannels = parseStringArray(
    raw.unobservedChannels,
    "effects.evidence.unobservedChannels",
    CHANNEL_SET
  ) as Channel[];
  if (raw.stateBuckets !== "exact" && raw.stateBuckets !== "unified") {
    throw new Error(
      'rhino-local: effects.evidence.stateBuckets must be "exact" or "unified"'
    );
  }
  return {
    stateBuckets: raw.stateBuckets,
    ambientState: parseJsonObject(
      raw.ambientState,
      "effects.evidence.ambientState"
    ),
    unbucketedState: parseArray(
      raw.unbucketedState,
      "effects.evidence.unbucketedState",
      parseUnbucketedMutation
    ),
    unobservedChannels,
  };
}

function parseUnbucketedMutation(
  raw: unknown,
  path: string
): UnbucketedStateMutation {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.operation !== "string") {
    throw new Error(`rhino-local: ${path}.operation must be a string`);
  }
  if (typeof raw.key !== "string") {
    throw new Error(`rhino-local: ${path}.key must be a string`);
  }
  const possibleBuckets = parseStringArray(
    raw.possibleBuckets,
    `${path}.possibleBuckets`,
    STATE_CHANNEL_SET
  ) as StateChannel[];
  if (possibleBuckets.length === 0) {
    throw new Error(`rhino-local: ${path}.possibleBuckets must not be empty`);
  }
  if (raw.operation === "added") {
    return {
      operation: "added",
      key: raw.key,
      after: parseJsonValue(raw.after, `${path}.after`),
      possibleBuckets,
    };
  }
  if (raw.operation === "changed") {
    return {
      operation: "changed",
      key: raw.key,
      before: parseJsonValue(raw.before, `${path}.before`),
      after: parseJsonValue(raw.after, `${path}.after`),
      possibleBuckets,
    };
  }
  if (raw.operation === "removed") {
    return {
      operation: "removed",
      key: raw.key,
      before: parseJsonValue(raw.before, `${path}.before`),
      possibleBuckets,
    };
  }
  throw new Error(
    `rhino-local: ${path}.operation must be added, changed, or removed`
  );
}

function parseStringArray(
  raw: unknown,
  path: string,
  allowed: ReadonlySet<string>
): string[] {
  if (!Array.isArray(raw) || raw.some((value) => typeof value !== "string")) {
    throw new Error(`rhino-local: ${path} must be an array of strings`);
  }
  const values = raw as string[];
  for (const value of values) {
    if (!allowed.has(value)) {
      throw new Error(`rhino-local: ${path} contains unknown value ${formatValue(value)}`);
    }
  }
  return [...new Set(values)];
}

function recordedChannelHasValues(
  effects: CompleteRecordedEffects,
  channel: Channel
): boolean {
  if (channel === "outcome") {
    return effects.outcome !== null;
  }
  if (channel === "sharedState" || channel === "transientState" || channel === "secureState" || channel === "sessionProperties") {
    return !deepEqual(effects[channel].initial, effects[channel].final);
  }
  return effects[channel].length > 0;
}

function parseStateBucket(raw: unknown, path: string): StateBucket {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (!("initial" in raw) || !("final" in raw)) {
    throw new Error(`rhino-local: ${path} must have initial and final objects`);
  }
  return {
    initial: parseJsonObject(raw.initial, `${path}.initial`),
    final: parseJsonObject(raw.final, `${path}.final`),
  };
}

function parseCallbackEffect(raw: unknown, path: string): CallbackEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.type !== "string" || raw.type.trim() === "") {
    throw new Error(`rhino-local: ${path}.type must be a non-empty string`);
  }
  const callback: CallbackEffect = { type: raw.type };
  for (const [key, value] of Object.entries(raw)) {
    if (key === "type") {
      continue;
    }
    callback[key] = parseJsonValue(value, `${path}.${key}`);
  }
  return callback;
}

function parseOpenidmEffect(raw: unknown, path: string): OpenidmEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.method !== "string" || !OPENIDM_METHOD_SET.has(raw.method)) {
    throw new Error(
      `rhino-local: ${path}.method must be one of ${OPENIDM_METHODS.join(", ")}`
    );
  }
  if (typeof raw.resource !== "string") {
    throw new Error(`rhino-local: ${path}.resource must be a string`);
  }
  const effect: OpenidmEffect = {
    method: raw.method as OpenidmMethod,
    resource: raw.resource,
  };
  if (raw.body !== undefined) {
    effect.body = parseJsonValue(raw.body, `${path}.body`);
  }
  if (raw.actionName !== undefined) {
    if (typeof raw.actionName !== "string") {
      throw new Error(`rhino-local: ${path}.actionName must be a string`);
    }
    effect.actionName = raw.actionName;
  }
  return effect;
}

function parseHttpEffect(raw: unknown, path: string): HttpEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.url !== "string") {
    throw new Error(`rhino-local: ${path}.url must be a string`);
  }
  if (typeof raw.method !== "string" || raw.method.trim() === "") {
    throw new Error(`rhino-local: ${path}.method must be a non-empty string`);
  }
  const effect: HttpEffect = { url: raw.url, method: raw.method };
  if (raw.body !== undefined) {
    effect.body = parseJsonValue(raw.body, `${path}.body`);
  }
  return effect;
}

function parseLogEffect(raw: unknown, path: string): LogEffect {
  if (!isPlainObject(raw)) {
    throw new Error(`rhino-local: ${path} is not an object`);
  }
  if (typeof raw.level !== "string" || !LOG_LEVEL_SET.has(raw.level)) {
    throw new Error(
      `rhino-local: ${path}.level must be one of ${LOG_LEVELS.join(", ")}`
    );
  }
  if (typeof raw.message !== "string") {
    throw new Error(`rhino-local: ${path}.message must be a string`);
  }
  return { level: raw.level as LogLevel, message: raw.message };
}

function parseArray<T>(
  raw: unknown,
  path: string,
  item: (value: unknown, itemPath: string) => T
): T[] {
  if (!Array.isArray(raw)) {
    throw new Error(`rhino-local: ${path} is not an array`);
  }
  return raw.map((value, index) => item(value, `${path}[${index}]`));
}

function formatSummary(kase: Case, mismatches: Mismatch[]): string {
  const plural = mismatches.length === 1 ? "" : "es";
  const header = `${JSON.stringify(kase.name)} failed (${mismatches.length} mismatch${plural}):`;
  const body = mismatches
    .map(
      (item) =>
        `${item.message}\n  expected: ${item.expected}\n  actual: ${item.actual}`
    )
    .join("\n");
  return `${header}\n${body}`;
}

function formatOpenidm(op: OpenidmEffect): string {
  const action =
    op.actionName === undefined ? "" : ` action=${formatValue(op.actionName)}`;
  const body = op.body === undefined ? "" : ` body=${formatValue(op.body)}`;
  return `${op.method} ${op.resource}${action}${body}`;
}

function formatOpenidmExpect(item: OpenidmExpect): string {
  const resource =
    typeof item.resource === "string"
      ? item.resource
      : String(item.resource);
  const action =
    item.actionName === undefined
      ? ""
      : ` action=${typeof item.actionName === "string" ? formatValue(item.actionName) : String(item.actionName)}`;
  const body = item.body === undefined ? "" : ` body=${formatValue(item.body)}`;
  return `${item.method} ${resource}${action}${body}`;
}

function formatHttp(op: HttpEffect): string {
  return `${op.method} ${op.url}`;
}

function formatHttpExpect(item: HttpExpect): string {
  const url = typeof item.url === "string" ? item.url : String(item.url);
  const method = item.method === undefined ? "" : `${item.method} `;
  const body = item.body === undefined ? "" : ` body=${formatValue(item.body)}`;
  return `${method}${url}${body}`;
}

function formatLog(op: LogEffect): string {
  return `${op.level} ${formatValue(op.message)}`;
}

function formatLogExpect(item: LogExpect): string {
  const level = item.level === undefined ? "" : `${item.level} `;
  const message =
    typeof item.message === "string"
      ? formatValue(item.message)
      : String(item.message);
  return `${level}${message}`;
}

function miss(
  channel: EvidenceChannel,
  path: string,
  expected: string,
  actual: string,
  message: string
): Mismatch {
  return { channel, path, expected, actual, message };
}
