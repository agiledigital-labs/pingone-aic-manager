/**
 * Built-in node types beyond the collectors and identity nodes in nodes.ts: Data Store Decision, the session
 * and cookie nodes, the one-time-password generator, Retry Limit Decision, Accept Terms and Conditions and the
 * CAPTCHA stub. Each is read from the node JSON as the tenant stores it and measured on a tenant
 * (docs/api/25-journey-runtime.md, "Built-in node types", 2026-11-25).
 */
import { createHmac, randomInt } from "node:crypto";
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
});
export const EXTRA_TYPES: readonly string[] = Object.keys(EXTRA_OUTCOMES);
/** Types whose outcome the harness cannot reproduce: they take it from `JourneyOptions.declaredOutcomes`. */
export const STUB_TYPES: readonly string[] = ["product-CaptchaNode"];

const SOURCE = "docs/api/25-journey-runtime.md (built-in node types, 2026-11-25)";
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
  if (type === "RetryLimitDecisionNode" && node.retryLimit !== undefined) {
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
    if (user.accountStatus !== undefined && user.accountStatus !== "active")
      this.ctx.missing("DataStoreDecisionNode over an account that is not active");
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
    const part = (value: JsonValue): string =>
      Buffer.from(JSON.stringify(value)).toString("base64url");
    const signing = `${part({ alg: "HS256", typ: "JWT" })}.${part({
      sub: typeof shared.username === "string" ? shared.username : null,
      iat: Math.floor(now / 1000),
      exp: Math.floor(now / 1000) + Math.round(maxLife * 3600),
    })}`;
    const value = `${signing}.${createHmac("sha256", Buffer.from(key, "base64")).update(signing).digest("base64url")}`;
    evidence.assumed(
      "SetPersistentCookieNode value is an HS256 JWT over username, iat and exp; its real claims are unmeasured",
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

  async run(
    type: string,
    id: string,
    config: NodeDocument,
    shared: Document,
    transient: Document,
    submitted: CallbackEffect[],
    defaultResource: string,
  ): Promise<NodeResult> {
    switch (type) {
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
