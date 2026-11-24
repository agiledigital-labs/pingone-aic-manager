/**
 * Built-in node types beyond the collectors and identity nodes in nodes.ts: Data Store Decision, the session
 * and cookie nodes, the one-time-password generator, Retry Limit Decision, Accept Terms and Conditions and the
 * CAPTCHA stub. Each is read from the node JSON as the tenant stores it and measured on a tenant
 * (docs/api/25-journey-runtime.md, "Built-in node types", 2026-11-25).
 */
import { randomInt } from "node:crypto";
import { mintCookie, readCookie } from "./cookie.ts";
import type { CallbackEffect } from "../case/types.ts";
import { EmulationFailure } from "../emulation.ts";
import { isDocument, own, setOwn } from "../idm/document.ts";
import type { IdmEmulator } from "../idm/emulator.ts";
import type { Evidence } from "../idm/provenance.ts";
import type { Document, RestResult } from "../idm/types.ts";
import type { JsonValue } from "../protocol.ts";
import type { NodeResult } from "./nodes.ts";
import type { NodeDocument, SessionDelta, TermsAndConditions } from "./types.ts";

/** Outcome vocabularies as `listOutcomes` reports them on the tenant (2026-11-25). */
export const EXTRA_OUTCOMES: Readonly<Record<string, readonly string[]>> = Object.freeze({
  DataStoreDecisionNode: ["true", "false"],
  SetSessionPropertiesNode: ["outcome"],
  SetPersistentCookieNode: ["outcome"],
  OneTimePasswordGeneratorNode: ["outcome"],
  RetryLimitDecisionNode: ["Retry", "Reject"],
  AcceptTermsAndConditionsNode: ["outcome"],
  "product-CaptchaNode": ["true", "false"],
  AnonymousUserNode: ["outcome"],
  SetSuccessUrlNode: ["outcome"],
  SetStateNode: ["outcome"],
  ModifyAuthLevelNode: ["outcome"],
  AuthLevelDecisionNode: ["true", "false"],
  MessageNode: ["true", "false"],
  ValidatedUsernameNode: ["outcome"],
  GetAuthenticatorAppNode: ["outcome"],
  OneTimePasswordCollectorDecisionNode: ["true", "false"],
  PersistentCookieDecisionNode: ["true", "false"],
  // Stubs (below): vocabularies as listOutcomes reports them with the type's template configuration.
  PushWaitNode: ["DONE", "EXITED"],
  PushResultVerifierNode: ["TRUE", "FALSE", "EXPIRED", "WAITING"],
  PushRegistrationNode: ["successOutcome", "failureOutcome", "timeoutOutcome"],
  "product-Saml2Node": ["ACCOUNT_EXISTS", "NO_ACCOUNT"],
  OneTimePasswordSmtpSenderNode: ["outcome"],
  OneTimePasswordSmsSenderNode: ["outcome"],
  DisplayUserNameNode: ["outcome"],
  AnonymousSessionUpgradeNode: ["outcome"],
  RecoveryCodeDisplayNode: ["outcome"],
  PollingWaitNode: ["DONE"],
  PushAuthenticationSenderNode: ["SENT", "NOT_REGISTERED", "SKIPPED"],
  ChoiceCollectorNode: [],
});
export const EXTRA_TYPES: readonly string[] = Object.keys(EXTRA_OUTCOMES);
/**
 * Types whose outcome the harness cannot reproduce: they take it from `JourneyOptions.declaredOutcomes`.
 * `ValidatedUsernameNode` is one only with `validateInput: true`, whose policy callback is not modelled.
 */
export const STUB_TYPES: readonly string[] = [
  "product-CaptchaNode",
  "ValidatedUsernameNode",
  "PushWaitNode",
  "PushResultVerifierNode",
  "PushRegistrationNode",
  "product-Saml2Node",
  "OneTimePasswordSmtpSenderNode",
  "OneTimePasswordSmsSenderNode",
  "DisplayUserNameNode",
  "AnonymousSessionUpgradeNode",
  "RecoveryCodeDisplayNode",
  "PollingWaitNode",
  "PushAuthenticationSenderNode",
];
/** Pure stubs: no behaviour at all, only a declared outcome. */
const PURE_STUBS = STUB_TYPES.filter(
  (type) => type !== "product-CaptchaNode" && type !== "ValidatedUsernameNode",
);
/** Vocabularies that depend on the node's configuration, measured with `listOutcomes` (2026-11-25). */
export function extraOutcomes(type: string, node: NodeDocument): readonly string[] | undefined {
  if (type === "ChoiceCollectorNode")
    return Array.isArray(node.choices)
      ? [...new Set(node.choices.filter((c): c is string => typeof c === "string"))]
      : [];
  if (type === "PollingWaitNode") return node.exitable === true ? ["DONE", "EXITED"] : ["DONE"];
  if (type === "PushAuthenticationSenderNode")
    return node.mandatory === true
      ? ["SENT", "NOT_REGISTERED"]
      : ["SENT", "NOT_REGISTERED", "SKIPPED"];
  return undefined;
}

const SOURCE = "docs/api/25-journey-runtime.md (built-in node types, 2026-11-25)";
const PAGE_CHILD_SOURCE = "docs/api/25-journey-runtime.md (Nodes as Page children, 2026-11-25)";
const APP_MESSAGE = "Get the app from the {{appleLink}} or on {{googleLink}}";
const APPLE_LINK = "https://apps.apple.com/app/pingid/id891247102";
const GOOGLE_LINK = "https://play.google.com/store/apps/details?id=prod.com.pingidentity.pingid";
const COOKIE_SOURCE = "docs/api/25-journey-runtime.md (Persistent cookie value, 2026-11-25)";
const SYSTEM_PROPERTIES = [
  "clientType",
  "FullLoginURL",
  "Principals",
  "XUSRef",
  "Locale",
  "amlbcookie",
  "UserId",
  "loginURL",
  "logoutHooks",
  "Service",
  "HostName",
  "UserToken",
  "IndexType",
  "webhooks",
  "UserProfile",
  "AuthLevel",
  "CharSet",
  "authInstant",
  "Host",
  "AMCtxId",
  "OidcSid",
  "sun.am.UniversalIdentifier",
  "Organization",
  "Principal",
];
const LOGIN_FAILURE = {
  status: 401,
  body: { code: 401, reason: "Unauthorized", message: "Login failure" },
} as const;

/** A node configuration the tenant refuses at write time; the emulator refuses it when the config loads. */
export function validateExtraNode(type: string, node: NodeDocument): void {
  if (type === "OneTimePasswordGeneratorNode" && node.length !== undefined) {
    if (typeof node.length !== "number" || !Number.isInteger(node.length) || node.length < 6)
      throw new RangeError(
        "400 Data validation failed for the attribute, One Time Password Length",
      );
  }
  // `{"$int": "&{esv.…}"}` is how the tenant stores an ESV-backed limit; it resolves at run time.
  if (
    type === "RetryLimitDecisionNode" &&
    node.retryLimit !== undefined &&
    !(isDocument(node.retryLimit) && typeof node.retryLimit.$int === "string")
  ) {
    if (
      typeof node.retryLimit !== "number" ||
      !Number.isInteger(node.retryLimit) ||
      node.retryLimit < 1
    )
      throw new RangeError(
        "400 Data validation failed for the attribute, Retry Limit. Reason: The value must be greater than zero",
      );
  }
  if (type === "SetSessionPropertiesNode") {
    const props = node.properties;
    const named = isDocument(props) ? Object.keys(props) : [];
    if (!named.length && node.maxSessionTime === undefined && node.maxIdleTime === undefined)
      throw new RangeError(
        "400 Invalid config: At least one of the following properties must be set: properties, maxSessionTime, maxIdleTime",
      );
    if (named.some((key) => SYSTEM_PROPERTIES.includes(key)))
      throw new RangeError(
        `400 Invalid config: Provided properties cannot include system session properties: [${SYSTEM_PROPERTIES.join(", ")}]`,
      );
  }
}

export type ExtraContext = {
  idm: IdmEmulator;
  evidence: Evidence;
  missing: (name: string) => never;
  now: () => number;
  terms?: TermsAndConditions;
  declared?: (nodeId: string) => string | undefined;
};

export class ExtraNodes {
  private readonly ctx: ExtraContext;
  constructor(ctx: ExtraContext) {
    this.ctx = ctx;
  }

  private async query(resource: string, attribute: string, value: string): Promise<Document[]> {
    const response: RestResult = await this.ctx.idm.hostRequest({
      type: "call",
      id: "journey-node",
      seq: 0,
      method: "query",
      args: [
        resource,
        {
          _queryFilter: `${attribute} eq ${JSON.stringify(value)}`,
          _fields: "_id,userName,password,accountStatus",
        },
      ],
    });
    for (const entry of response.provenance)
      this.ctx.evidence.add(entry.kind, entry.behavior, entry.source);
    if (response.emulation) throw new EmulationFailure(response.emulation);
    if (response.status >= 400) this.ctx.missing(`identity lookup error: ${response.status}`);
    if (!isDocument(response.body) || !Array.isArray(response.body.result))
      this.ctx.missing("identity lookup result shape");
    return response.body.result.filter(isDocument);
  }

  private async dataStore(
    shared: Document,
    transient: Document,
    resource: string,
  ): Promise<NodeResult> {
    const { evidence } = this.ctx;
    // Measured: the username is read from shared state only (a transient username aborts), the password from
    // either bucket; a missing one aborts the journey instead of taking the false outcome.
    const username = own(shared, "username");
    const password = Object.hasOwn(transient, "password")
      ? transient.password
      : own(shared, "password");
    evidence.measured(
      "DataStoreDecisionNode reads shared username (a transient-only username aborts with 401 Login failure) and a password from either bucket",
      SOURCE,
    );
    if (username === undefined || password === undefined)
      return {
        outcome: null,
        callbacks: [],
        abort: structuredClone(LOGIN_FAILURE),
      };
    if (typeof username !== "string" || typeof password !== "string")
      this.ctx.missing("DataStoreDecisionNode with a non-string username or password");
    if (username === "") this.ctx.missing("DataStoreDecisionNode with an empty username");
    const matches = await this.query(resource, "userName", username);
    if (matches.length > 1)
      this.ctx.missing("DataStoreDecisionNode lookup matched multiple records");
    evidence.measured(
      "DataStoreDecisionNode matches userName case-insensitively on the tree identity resource only (a mail address is not a username) and writes no state",
      SOURCE,
    );
    const user = matches[0];
    if (!user) return { outcome: "false", callbacks: [] };
    evidence.measured(
      "DataStoreDecisionNode does not read accountStatus: any status, in any case, takes the password verdict",
      "docs/api/25-journey-runtime.md (DataStoreDecisionNode and accountStatus, 2026-11-25)",
    );
    if (typeof user.password !== "string")
      this.ctx.missing("DataStoreDecisionNode over a user with no password");
    evidence.measured(
      "DataStoreDecisionNode takes false for a wrong, empty or unknown-user password and true for the user's own",
      SOURCE,
    );
    return {
      outcome: this.ctx.idm.verifyPassword(user.password, password) ? "true" : "false",
      callbacks: [],
    };
  }

  private sessionProperties(config: NodeDocument): NodeResult {
    const { evidence } = this.ctx;
    const delta: SessionDelta = {};
    const props = config.properties;
    if (props !== undefined) {
      if (!isDocument(props)) this.ctx.missing("SetSessionPropertiesNode properties configuration");
      const out: Record<string, string> = {};
      for (const [key, value] of Object.entries(props)) {
        if (typeof value !== "string")
          this.ctx.missing(`SetSessionPropertiesNode property value type: ${key}`);
        out[key] = value;
      }
      if (Object.keys(out).length) delta.properties = out;
    }
    for (const key of ["maxIdleTime", "maxSessionTime"] as const) {
      const value = config[key];
      if (value === undefined) continue;
      if (typeof value !== "number")
        this.ctx.missing(`SetSessionPropertiesNode ${key} configuration`);
      delta[key] = value;
    }
    evidence.measured(
      "SetSessionPropertiesNode adds each configured property to the session, and maxIdleTime / maxSessionTime (minutes) set its expirations",
      SOURCE,
    );
    return { outcome: "outcome", callbacks: [], session: delta };
  }

  private persistentCookie(config: NodeDocument, shared: Document): NodeResult {
    const { evidence } = this.ctx;
    const name =
      typeof config.persistentCookieName === "string" ? config.persistentCookieName : "session-jwt";
    const maxLife = typeof config.maxLife === "number" ? config.maxLife : 5;
    const sameSite = typeof config.sameSite === "string" ? config.sameSite.toLowerCase() : "lax";
    if (!["lax", "strict", "none"].includes(sameSite))
      this.ctx.missing(`SetPersistentCookieNode sameSite: ${sameSite}`);
    const key =
      typeof config.hmacSigningKey === "string" && config.hmacSigningKey !== ""
        ? config.hmacSigningKey
        : undefined;
    if (key === undefined) {
      if (typeof config.secretLabelIdentifier === "string" && config.secretLabelIdentifier !== "")
        this.ctx.missing("SetPersistentCookieNode signing through a secret label");
      evidence.measured(
        "SetPersistentCookieNode with no signing key configured fails the journey with an empty 500",
        SOURCE,
      );
      return {
        outcome: null,
        callbacks: [],
        abort: { status: 500, body: null, rawBody: "" },
      };
    }
    const now = this.ctx.now();
    const value = mintCookie(
      {
        sub: typeof shared.username === "string" ? shared.username : null,
        iat: Math.floor(now / 1000),
        exp: Math.floor(now / 1000) + Math.round(maxLife * 3600),
      },
      key,
    );
    evidence.measured(
      "SetPersistentCookieNode's value is an HS256 JWS (typ JWT, cty JWT) over a five-part encrypted JWE; the emulator's inner part is its own A128CBC-HS256 seal over sub, iat and exp, which are the emulator's claim names (the tenant's are unreadable)",
      COOKIE_SOURCE,
    );
    evidence.assumed(
      "SetPersistentCookieNode idleTimeout has no measured effect and is not modelled",
    );
    evidence.measured(
      "SetPersistentCookieNode sets a cookie named persistentCookieName expiring maxLife hours ahead, always Secure, HttpOnly per useHttpOnlyCookie, SameSite as configured",
      SOURCE,
    );
    return {
      outcome: "outcome",
      callbacks: [],
      session: {
        cookie: {
          name,
          value,
          expires: new Date(now + maxLife * 3600_000).toUTCString(),
          path: "/",
          secure: true,
          httpOnly: config.useHttpOnlyCookie !== false,
          sameSite: sameSite as "lax" | "strict" | "none",
        },
      },
    };
  }

  private otp(config: NodeDocument, shared: Document): NodeResult {
    const length = typeof config.length === "number" ? config.length : 8;
    this.ctx.evidence.measured(
      "OneTimePasswordGeneratorNode writes shared oneTimePassword (digits, leading zeros kept, length 6 to 8) and oneTimePasswordTimestamp (epoch seconds, a number); a length of 9 or more fails the journey with 401 Login failure",
      SOURCE,
    );
    if (length > 8)
      return {
        outcome: null,
        callbacks: [],
        abort: structuredClone(LOGIN_FAILURE),
      };
    let digits = "";
    for (let i = 0; i < length; i++) digits += String(randomInt(10));
    setOwn(shared, "oneTimePassword", digits);
    setOwn(shared, "oneTimePasswordTimestamp", Math.floor(this.ctx.now() / 1000));
    return { outcome: "outcome", callbacks: [] };
  }

  private retryLimit(config: NodeDocument, shared: Document, id: string): NodeResult {
    const limit = typeof config.retryLimit === "number" ? config.retryLimit : 3;
    const key = `${id}.retryCount`;
    const seen = own(shared, key);
    if (seen !== undefined && typeof seen !== "number")
      this.ctx.missing("RetryLimitDecisionNode non-numeric retryCount");
    const count = seen ?? 0;
    this.ctx.evidence.measured(
      "RetryLimitDecisionNode counts visits in shared <nodeId>.retryCount, takes Retry for the first retryLimit visits and Reject after, leaving the count at the limit",
      SOURCE,
    );
    this.ctx.evidence.measured(
      "RetryLimitDecisionNode incrementUserAttributeOnFailure changed no IDM-visible attribute of the identified user",
      SOURCE,
    );
    if (count >= limit) return { outcome: "Reject", callbacks: [] };
    setOwn(shared, key, count + 1);
    return { outcome: "Retry", callbacks: [] };
  }

  private terms(shared: Document, submitted: CallbackEffect[]): NodeResult {
    const { terms, evidence } = this.ctx;
    if (!terms)
      this.ctx.missing("AcceptTermsAndConditionsNode needs JourneyOptions.termsAndConditions");
    const callback = (): CallbackEffect => ({
      type: "TermsAndConditionsCallback",
      version: terms.version,
      terms: terms.terms,
      createDate: terms.createDate,
    });
    evidence.measured(
      "AcceptTermsAndConditionsNode sends one TermsAndConditionsCallback (version, terms, createDate) whose input starts false, and sends it again on any answer but true",
      SOURCE,
    );
    if (!submitted.length) return { outcome: null, callbacks: [callback()] };
    const answer = submitted.length === 1 ? submitted[0] : undefined;
    if (answer?.type !== "TermsAndConditionsCallback")
      this.ctx.missing("AcceptTermsAndConditionsNode callback submission");
    const value = answer.value;
    if (value === false || value === "false" || value === "")
      return { outcome: null, callbacks: [callback()] };
    if (value !== true && value !== "true")
      this.ctx.missing(`AcceptTermsAndConditionsNode answer: ${JSON.stringify(value)}`);
    const accepted = new Date(this.ctx.now()).toISOString().replace("Z", "000000Z");
    setOwn(
      shared,
      "termsAccepted",
      `{ "acceptDate": "${accepted}", "termsVersion": "${terms.version}" }`,
    );
    setOwn(shared, "termsVersion", terms.version);
    evidence.measured(
      "AcceptTermsAndConditionsNode accepts true or the string true and writes shared termsAccepted (a JSON string of acceptDate and termsVersion) and termsVersion",
      SOURCE,
    );
    evidence.assumed(
      "AcceptTermsAndConditionsNode acceptDate carries milliseconds then zeros; the tenant's nanoseconds are not reproducible",
    );
    return { outcome: "outcome", callbacks: [] };
  }

  /**
   * CAPTCHA verification calls the provider, which the harness cannot reproduce, so the callback is measured and
   * the verdict is declared by the caller. A tenant given a rejected response ends the journey with 401.
   */
  private captcha(config: NodeDocument, id: string, submitted: CallbackEffect[]): NodeResult {
    const { evidence } = this.ctx;
    if (config.reCaptchaV3 === true) this.ctx.missing("product-CaptchaNode reCaptchaV3: true");
    for (const key of ["siteKey", "apiUri", "divClass"] as const)
      if (typeof config[key] !== "string")
        this.ctx.missing(`product-CaptchaNode ${key} configuration`);
    if (typeof config.disableSubmission !== "boolean")
      this.ctx.missing("product-CaptchaNode disableSubmission configuration");
    evidence.measured(
      "product-CaptchaNode sends one ReCaptchaCallback (recaptchaSiteKey, captchaApiUri, captchaDivClass, reCaptchaV3, disableSubmission from the node) with an empty string input",
      SOURCE,
    );
    if (!submitted.length)
      return {
        outcome: null,
        callbacks: [
          {
            type: "ReCaptchaCallback",
            recaptchaSiteKey: config.siteKey as string,
            captchaApiUri: config.apiUri as string,
            captchaDivClass: config.divClass as string,
            reCaptchaV3: false,
            disableSubmission: config.disableSubmission,
          },
        ],
      };
    if (submitted.length !== 1 || submitted[0]?.type !== "ReCaptchaCallback")
      this.ctx.missing("product-CaptchaNode callback submission");
    const outcome = this.ctx.declared?.(id);
    if (outcome === undefined)
      this.ctx.missing(
        "product-CaptchaNode verification outcome; declare it in JourneyOptions.declaredOutcomes",
      );
    evidence.assumed(`declared by test: product-CaptchaNode/${id} -> ${outcome}`);
    return { outcome, callbacks: [] };
  }

  private anonymous(config: NodeDocument, shared: Document): NodeResult {
    const name = config.anonymousUserName === undefined ? "anonymous" : config.anonymousUserName;
    if (typeof name !== "string") this.ctx.missing("AnonymousUserNode anonymousUserName type");
    setOwn(shared, "username", name);
    this.ctx.evidence.measured(
      "AnonymousUserNode writes shared username as anonymousUserName (default anonymous), and a journey ending there succeeds",
      SOURCE,
    );
    return { outcome: "outcome", callbacks: [] };
  }

  private successUrl(config: NodeDocument, shared: Document): NodeResult {
    if (config.successUrl !== undefined && typeof config.successUrl !== "string")
      this.ctx.missing("SetSuccessUrlNode successUrl type");
    setOwn(shared, "successUrl", config.successUrl ?? null);
    this.ctx.evidence.measured(
      "SetSuccessUrlNode writes shared successUrl, which the success response carries; with none configured the response says null",
      SOURCE,
    );
    return { outcome: "outcome", callbacks: [] };
  }

  private setState(config: NodeDocument, shared: Document): NodeResult {
    const attributes = config.attributes;
    if (attributes !== undefined && !isDocument(attributes))
      this.ctx.missing("SetStateNode attributes configuration");
    for (const [key, value] of Object.entries(attributes ?? {})) {
      if (typeof value !== "string") this.ctx.missing(`SetStateNode attribute value type: ${key}`);
      setOwn(shared, key, value);
    }
    this.ctx.evidence.measured(
      "SetStateNode writes each configured attribute to shared state as the literal string (no ${...} expansion), overwriting an existing key",
      SOURCE,
    );
    return { outcome: "outcome", callbacks: [] };
  }

  private authLevel(config: NodeDocument, shared: Document, decide: boolean): NodeResult {
    const level = shared.authLevel;
    if (typeof level !== "number") this.ctx.missing("authLevel that is not a number");
    const key = decide ? "authLevelRequirement" : "authLevelIncrement";
    const amount = config[key] === undefined ? 0 : config[key];
    if (typeof amount !== "number") this.ctx.missing(`${key} configuration`);
    if (!decide) {
      setOwn(shared, "authLevel", level + amount);
      this.ctx.evidence.measured(
        "ModifyAuthLevelNode adds authLevelIncrement (negative allowed) to the numeric shared authLevel",
        SOURCE,
      );
      return { outcome: "outcome", callbacks: [] };
    }
    this.ctx.evidence.measured(
      "AuthLevelDecisionNode takes true when shared authLevel is at least authLevelRequirement, else false",
      SOURCE,
    );
    return { outcome: level >= amount ? "true" : "false", callbacks: [] };
  }

  private message(config: NodeDocument, submitted: CallbackEffect[]): NodeResult {
    const text = (key: string, fallback: string): string => {
      const map = config[key];
      if (map === undefined) return fallback;
      if (!isDocument(map)) this.ctx.missing(`MessageNode ${key} configuration`);
      const first = Object.values(map)[0];
      if (first === undefined) return fallback;
      if (typeof first !== "string") this.ctx.missing(`MessageNode ${key} value type`);
      return first;
    };
    this.ctx.evidence.measured(
      'MessageNode sends a TextOutputCallback (messageType "0") and a ConfirmationCallback (options yes then no, default option 1); answer 0 takes true, any other index false; no state is written',
      SOURCE,
    );
    this.ctx.evidence.assumed(
      "MessageNode uses the first entry of each language map; only single-entry maps were measured",
    );
    if (!submitted.length)
      return {
        outcome: null,
        callbacks: [
          {
            type: "TextOutputCallback",
            message: text("message", "Default message"),
            messageType: "0",
          },
          {
            type: "ConfirmationCallback",
            prompt: "",
            messageType: 0,
            options: [text("messageYes", "Yes"), text("messageNo", "No")],
            optionType: -1,
            defaultOption: 1,
          },
        ],
      };
    const answer = submitted.length === 2 ? submitted[1] : undefined;
    if (answer?.type !== "ConfirmationCallback")
      this.ctx.missing("MessageNode callback submission");
    const index = typeof answer.value === "string" ? Number(answer.value) : answer.value;
    if (typeof index !== "number" || !Number.isInteger(index))
      this.ctx.missing(`MessageNode answer: ${JSON.stringify(answer.value)}`);
    return { outcome: index === 0 ? "true" : "false", callbacks: [] };
  }

  /**
   * Measured standing alone and as a Page child (doc 25, "Nodes as Page children", 2026-11-25): a
   * TextOutputCallback of type "4" (a script) holding the message, then a ConfirmationCallback with the one
   * continue option; any answer takes the one outcome and no state is written.
   */
  private getAuthenticatorApp(config: NodeDocument, submitted: CallbackEffect[]): NodeResult {
    const first = (key: string, fallback: string): string => {
      const map = config[key];
      if (map === undefined) return fallback;
      if (!isDocument(map)) this.ctx.missing(`GetAuthenticatorAppNode ${key} configuration`);
      const value = Object.values(map)[0];
      if (value === undefined) return fallback;
      if (typeof value !== "string") this.ctx.missing(`GetAuthenticatorAppNode ${key} value type`);
      return value;
    };
    const link = (key: "appleLink" | "googleLink", fallback: string): string => {
      const value = config[key] === undefined ? fallback : config[key];
      if (typeof value !== "string" || /['"\\\s]/.test(value))
        this.ctx.missing(`GetAuthenticatorAppNode ${key} that is not a plain URL`);
      return value;
    };
    const template = first("message", APP_MESSAGE);
    if (/\{\{(?!appleLink\}\}|googleLink\}\})/.test(template) || /["\\\r\n]/.test(template))
      this.ctx.missing("GetAuthenticatorAppNode message with a wildcard other than the two links, or a quote, backslash or line break");
    const html = template
      .replaceAll("{{appleLink}}", `<a target='_blank' href='${link("appleLink", APPLE_LINK)}'>Apple App Store</a>`)
      .replaceAll("{{googleLink}}", `<a target='_blank' href='${link("googleLink", GOOGLE_LINK)}'>Google Play Store</a>`);
    this.ctx.evidence.measured(
      'GetAuthenticatorAppNode sends a TextOutputCallback (messageType "4") of the script document.getElementById("callback_0").innerHTML="<center>…</center>" with {{appleLink}} and {{googleLink}} expanded to anchors labelled Apple App Store and Google Play Store, and a ConfirmationCallback with the one continueLabel option; any answer takes outcome and no state is written',
      PAGE_CHILD_SOURCE,
    );
    this.ctx.evidence.assumed(
      "GetAuthenticatorAppNode uses the first entry of each language map; the {{appleLabel}} and {{googleLabel}} wildcards are unmeasured",
    );
    if (!submitted.length)
      return {
        outcome: null,
        callbacks: [
          {
            type: "TextOutputCallback",
            message: `document.getElementById("callback_0").innerHTML="<center>${html}</center>"`,
            messageType: "4",
          },
          {
            type: "ConfirmationCallback",
            prompt: "",
            messageType: 0,
            options: [first("continueLabel", "Continue")],
            optionType: -1,
            defaultOption: 0,
          },
        ],
      };
    const answer = submitted.length === 2 ? submitted[1] : undefined;
    if (answer?.type !== "ConfirmationCallback" || typeof answer.value !== "number")
      this.ctx.missing("GetAuthenticatorAppNode callback submission");
    return { outcome: "outcome", callbacks: [] };
  }

  private choice(config: NodeDocument, submitted: CallbackEffect[]): NodeResult {
    const choices = config.choices;
    if (!Array.isArray(choices) || !choices.length || choices.some((c) => typeof c !== "string"))
      this.ctx.missing("ChoiceCollectorNode choices configuration");
    const list = choices as string[];
    this.ctx.evidence.measured(
      "ChoiceCollectorNode sends one ChoiceCallback (prompt, choices, defaultChoice index; 0 when the default is absent or not a choice) and takes the outcome named by the answered index; an index outside the list, or no prompt, fails the journey with 401 Login failure; no state is written",
      SOURCE,
    );
    if (typeof config.prompt !== "string")
      return { outcome: null, callbacks: [], abort: structuredClone(LOGIN_FAILURE) };
    if (!submitted.length) {
      const preferred =
        typeof config.defaultChoice === "string" ? list.indexOf(config.defaultChoice) : -1;
      return {
        outcome: null,
        callbacks: [
          {
            type: "ChoiceCallback",
            prompt: config.prompt,
            choices: list,
            defaultChoice: Math.max(preferred, 0),
          },
        ],
      };
    }
    const answer = submitted.length === 1 ? submitted[0] : undefined;
    if (answer?.type !== "ChoiceCallback")
      this.ctx.missing("ChoiceCollectorNode callback submission");
    const index = typeof answer.value === "string" ? Number(answer.value) : answer.value;
    if (typeof index !== "number" || !Number.isInteger(index) || index < 0 || index >= list.length)
      return { outcome: null, callbacks: [], abort: structuredClone(LOGIN_FAILURE) };
    return { outcome: list[index] ?? null, callbacks: [] };
  }

  private validatedUsername(
    config: NodeDocument,
    shared: Document,
    id: string,
    submitted: CallbackEffect[],
  ): NodeResult {
    if (config.validateInput !== false) {
      // The policy callback carries the tenant's policy set and re-sends on policy failures; not modelled.
      const outcome = this.ctx.declared?.(id);
      if (outcome === undefined)
        this.ctx.missing(
          "ValidatedUsernameNode validateInput: true; declare its outcome in JourneyOptions.declaredOutcomes",
        );
      this.ctx.evidence.assumed(`declared by test: ValidatedUsernameNode/${id} -> ${outcome}`);
      return { outcome, callbacks: [] };
    }
    if (config.usernameAttribute !== "userName")
      this.ctx.missing("ValidatedUsernameNode usernameAttribute other than userName");
    this.ctx.evidence.measured(
      "ValidatedUsernameNode with validateInput false sends a NameCallback (prompt User Name), accepts any answer, and writes shared username and objectAttributes.userName; an empty answer writes nothing",
      SOURCE,
    );
    if (!submitted.length)
      return {
        outcome: null,
        callbacks: [{ type: "NameCallback", prompt: "User Name" }],
      };
    const answer = submitted.length === 1 ? submitted[0] : undefined;
    if (answer?.type !== "NameCallback" || typeof answer.value !== "string")
      this.ctx.missing("ValidatedUsernameNode callback submission");
    if (answer.value !== "") {
      setOwn(shared, "username", answer.value);
      setOwn(shared, "objectAttributes", { userName: answer.value });
    }
    return { outcome: "outcome", callbacks: [] };
  }

  private otpCollector(
    config: NodeDocument,
    shared: Document,
    submitted: CallbackEffect[],
  ): NodeResult {
    const expiry = config.passwordExpiryTime === undefined ? 5 : config.passwordExpiryTime;
    if (typeof expiry !== "number") this.ctx.missing("passwordExpiryTime configuration");
    this.ctx.evidence.measured(
      "OneTimePasswordCollectorDecisionNode sends a PasswordCallback (prompt One Time Password), re-sends on an empty answer, and takes true only for the shared oneTimePassword answered within passwordExpiryTime minutes of oneTimePasswordTimestamp; a wrong, expired or absent password is false",
      SOURCE,
    );
    this.ctx.evidence.assumed(
      "OneTimePasswordCollectorDecisionNode's expiry boundary (exactly passwordExpiryTime minutes) is unmeasured",
    );
    if (!submitted.length)
      return {
        outcome: null,
        callbacks: [{ type: "PasswordCallback", prompt: "One Time Password" }],
      };
    const answer = submitted.length === 1 ? submitted[0] : undefined;
    if (answer?.type !== "PasswordCallback" || typeof answer.value !== "string")
      this.ctx.missing("OneTimePasswordCollectorDecisionNode callback submission");
    if (answer.value === "")
      return {
        outcome: null,
        callbacks: [{ type: "PasswordCallback", prompt: "One Time Password" }],
      };
    const otp = own(shared, "oneTimePassword");
    const stamp = own(shared, "oneTimePasswordTimestamp");
    const fresh = typeof stamp === "number" && this.ctx.now() / 1000 - stamp <= expiry * 60;
    return {
      outcome: typeof otp === "string" && otp === answer.value && fresh ? "true" : "false",
      callbacks: [],
    };
  }

  private async persistentCookieDecision(
    config: NodeDocument,
    shared: Document,
    resource: string,
    cookies: Readonly<Record<string, string>> | undefined,
  ): Promise<NodeResult> {
    const { evidence } = this.ctx;
    if (config.enforceClientIp === true)
      this.ctx.missing("PersistentCookieDecisionNode enforceClientIp: true");
    const name =
      typeof config.persistentCookieName === "string" ? config.persistentCookieName : "session-jwt";
    const key =
      typeof config.hmacSigningKey === "string" && config.hmacSigningKey !== ""
        ? config.hmacSigningKey
        : undefined;
    if (key === undefined) this.ctx.missing("PersistentCookieDecisionNode without hmacSigningKey");
    evidence.measured(
      "PersistentCookieDecisionNode takes false for no cookie, another signing key, another cookie name or an altered cookie, aborts with 401 for a value that is not a JWT, and on true writes shared username",
      SOURCE,
    );
    const value = cookies?.[name];
    if (value === undefined) return { outcome: "false", callbacks: [] };
    const reading = readCookie(value, key);
    evidence.measured(
      "PersistentCookieDecisionNode: a value with no dot (or empty), or whose signed payload has none, aborts with 401; two parts, another signing key, or an inner part that does not open take false; the inner part, not the signing key, names the user",
      COOKIE_SOURCE,
    );
    if (reading.kind === "unmeasured")
      this.ctx.missing(`PersistentCookieDecisionNode over ${reading.what}`);
    if (reading.kind === "abort")
      return { outcome: null, callbacks: [], abort: structuredClone(LOGIN_FAILURE) };
    if (reading.kind === "false") return { outcome: "false", callbacks: [] };
    const claims = reading.claims;
    if (claims.sub === null) return { outcome: "false", callbacks: [] };
    evidence.assumed(
      "PersistentCookieDecisionNode's rule for an expired cookie, and idleTimeout, are unmeasured; a cookie past its exp is taken as false",
    );
    if (claims.exp * 1000 < this.ctx.now()) return { outcome: "false", callbacks: [] };
    // Measured: for a user the identity store holds, shared username becomes the user's _id; for a name it does not hold, it is the name.
    const matches = await this.query(resource, "userName", claims.sub);
    if (matches.length > 1)
      this.ctx.missing("PersistentCookieDecisionNode lookup matched multiple records");
    const id = matches[0]?._id;
    evidence.measured(
      "PersistentCookieDecisionNode writes shared username as the cookie user's identity _id when the user exists, else the name from the cookie",
      SOURCE,
    );
    setOwn(shared, "username", typeof id === "string" ? id : claims.sub);
    return { outcome: "true", callbacks: [] };
  }

  private stub(type: string, id: string): NodeResult {
    const outcome = this.ctx.declared?.(id);
    if (outcome === undefined)
      this.ctx.missing(
        `${type} needs infrastructure the harness cannot model; declare its outcome in JourneyOptions.declaredOutcomes`,
      );
    this.ctx.evidence.assumed(`declared by test: ${type}/${id} -> ${outcome}`);
    return { outcome, callbacks: [] };
  }

  async run(
    type: string,
    id: string,
    config: NodeDocument,
    shared: Document,
    transient: Document,
    submitted: CallbackEffect[],
    defaultResource: string,
    cookies?: Readonly<Record<string, string>>,
  ): Promise<NodeResult> {
    if (PURE_STUBS.includes(type)) return this.stub(type, id);
    switch (type) {
      case "AnonymousUserNode":
        return this.anonymous(config, shared);
      case "SetSuccessUrlNode":
        return this.successUrl(config, shared);
      case "SetStateNode":
        return this.setState(config, shared);
      case "ModifyAuthLevelNode":
        return this.authLevel(config, shared, false);
      case "AuthLevelDecisionNode":
        return this.authLevel(config, shared, true);
      case "MessageNode":
        return this.message(config, submitted);
      case "GetAuthenticatorAppNode":
        return this.getAuthenticatorApp(config, submitted);
      case "ChoiceCollectorNode":
        return this.choice(config, submitted);
      case "ValidatedUsernameNode":
        return this.validatedUsername(config, shared, id, submitted);
      case "OneTimePasswordCollectorDecisionNode":
        return this.otpCollector(config, shared, submitted);
      case "PersistentCookieDecisionNode":
        return this.persistentCookieDecision(config, shared, defaultResource, cookies);
      case "DataStoreDecisionNode":
        return this.dataStore(shared, transient, defaultResource);
      case "SetSessionPropertiesNode":
        return this.sessionProperties(config);
      case "SetPersistentCookieNode":
        return this.persistentCookie(config, shared);
      case "OneTimePasswordGeneratorNode":
        return this.otp(config, shared);
      case "RetryLimitDecisionNode":
        return this.retryLimit(config, shared, id);
      case "AcceptTermsAndConditionsNode":
        return this.terms(shared, submitted);
      case "product-CaptchaNode":
        return this.captcha(config, id, submitted);
      default:
        return this.ctx.missing(`node type: ${type}`);
    }
  }
}

/**
 * A node type built in the console's node designer, as `GET /am/json/node-designer/node-type` stores it
 * (docs/api/25-journey-runtime.md, "Custom node types"). A tree node of it has `nodeType` "designer-<serviceName>".
 */
export type CustomNodeType = {
  readonly outcomes: readonly string[];
  readonly script: string;
  readonly errorOutcome?: boolean;
  readonly inputs?: readonly string[];
  readonly outputs?: readonly string[];
  readonly displayName?: string;
  readonly properties?: Readonly<
    Record<
      string,
      { readonly type?: string; readonly required?: boolean; readonly multivalued?: boolean }
    >
  >;
  readonly [key: string]: unknown;
};
export const SCRIPT_ERROR_OUTCOME = "Script Error";
/** `listOutcomes` for a custom type: its outcomes, plus "Script Error" when it has an error outcome. */
export function customOutcomes(def: CustomNodeType): readonly string[] {
  return def.errorOutcome === true ? [...def.outcomes, SCRIPT_ERROR_OUTCOME] : def.outcomes;
}
/**
 * The decision script a custom node runs as: its script with `properties` bound from the instance and the
 * assigned `outcome` routed. Absent properties follow the measured shape: a multivalued one is an empty array,
 * any other is undefined. A missing required property is `undefined` here and is the caller's 401.
 */
export function customNodeSource(
  def: CustomNodeType,
  node: NodeDocument,
  missing: (name: string) => never,
): { source: string } | { abort: true } {
  const inputs = def.inputs ?? [];
  if (inputs.some((name) => name !== "*"))
    missing(
      "custom node type with named inputs (reads of other state keys are restricted on the tenant)",
    );
  const declared = def.properties ?? {};
  const properties: Record<string, JsonValue> = {};
  for (const [name, spec] of Object.entries(declared)) {
    const value = node[name];
    if (value === undefined) {
      if (spec.required === true) return { abort: true };
      if (spec.multivalued === true) properties[name] = [];
      continue;
    }
    // A bare placeholder in a BOOLEAN or NUMBER property is refused at write time (measured), so a
    // value of another type than the property's was never stored: refuse rather than coerce.
    const wanted = { BOOLEAN: "boolean", NUMBER: "number", STRING: "string" }[String(spec.type)];
    const given = spec.multivalued === true ? undefined : typeof value;
    if (wanted !== undefined && given !== undefined && given !== wanted)
      missing(`custom node property ${name} typed ${String(spec.type)} holding a ${given}`);
    properties[name] = value as JsonValue;
  }
  const allowed = JSON.stringify(def.outcomes);
  const failure =
    def.errorOutcome === true ? `outcome = ${JSON.stringify(SCRIPT_ERROR_OUTCOME)};` : "throw e;";
  const source = [
    `var properties = ${JSON.stringify(properties)};`,
    "var outcome;",
    "try {",
    def.script,
    "} catch (e) {",
    failure,
    "}",
    `if (${allowed}.indexOf(outcome) < 0 && outcome !== ${JSON.stringify(SCRIPT_ERROR_OUTCOME)}) {`,
    def.errorOutcome === true
      ? `  outcome = ${JSON.stringify(SCRIPT_ERROR_OUTCOME)};`
      : '  throw "custom node outcome";',
    "}",
    "action.goTo(outcome);",
  ].join("\n");
  return { source };
}
