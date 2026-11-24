/** Collector and identity nodes; managed policy and writes stay in the shared IDM core. */
import type { CallbackEffect } from "../case/types.ts";
import { EmulationFailure } from "../emulation.ts";
import { isDocument, own, setOwn } from "../idm/document.ts";
import type { IdmEmulator } from "../idm/emulator.ts";
import type { Evidence } from "../idm/provenance.ts";
import type { Document, PropertyConfig, RestResult } from "../idm/types.ts";
import type { HostCall, JsonValue } from "../protocol.ts";
import { EXTRA_TYPES, ExtraNodes, type ExtraContext } from "./extra-nodes.ts";
import type {
  AttributePolicy,
  AuthenticateResult,
  NodeDocument,
  PasswordPolicy,
  SessionDelta,
} from "./types.ts";
export const EMULATED = [
  "ScriptedDecisionNode",
  "InnerTreeEvaluatorNode",
  "PageNode",
  "UsernameCollectorNode",
  "PasswordCollectorNode",
  "AttributeCollectorNode",
  "ValidatedPasswordNode",
  "IdentifyExistingUserNode",
  "CreateObjectNode",
  "PatchObjectNode",
  ...EXTRA_TYPES,
];
export type NodeResult = {
  /** Effects on the session AM creates when the journey succeeds. */
  session?: SessionDelta;
  outcome: string | null;
  callbacks: CallbackEffect[];
  /** A Page's resolved `pageHeader` / `pageDescription`, sent beside its callbacks. */
  header?: string;
  description?: string;
  abort?: Omit<AuthenticateResult, "provenance">;
  /**
   * The script's `auditEntryDetail` (string or map), for the node's completion record. A Page's
   * scripted child's is dropped: assignment inside a Page child is unmeasured.
   */
  audit?: JsonValue;
};
const SOURCE = "docs/api/25-journey-runtime.md (measurement agent, 2026-11-02)";
const PRECEDENCE_SOURCE =
  "docs/api/25-journey-runtime.md (Identify Existing User precedence, 2026-11-04)";
const BLANK_SOURCE =
  "docs/api/25-journey-runtime.md (A blank or null lookup value, and letter case, 2026-11-04)";
const IGNORED_SOURCE =
  "docs/api/25-journey-runtime.md (Patch Object ignoredFields, 2026-11-25)";
const EMPTY_SOURCE =
  "docs/api/25-journey-runtime.md (Identify Existing User with no identity value, 2026-11-25)";
/** Writes `value` at a slash path, creating the intermediate objects (a sibling path shares them). */
function setNested(target: Document, path: string[], value: JsonValue): void {
  const [head, ...rest] = path;
  if (head === undefined) return;
  if (!rest.length) return setOwn(target, head, value);
  const existing = own(target, head);
  const child: Document = isDocument(existing) ? existing : {};
  setNested(child, rest, value);
  setOwn(target, head, child);
}
const VALIDATE_SOURCE =
  "docs/api/25-journey-runtime.md (Attribute Collector validateInputs, 2026-11-25)";
// Only "not-an-email" failing and an ordinary address passing were measured; the rule is a guess.
const looksLikeEmail = (value: string): boolean =>
  /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(value);
const PASSWORD_SOURCE =
  "docs/api/25-journey-runtime.md (Validated password, 2026-11-25)";
export class JourneyNodes {
  private readonly idm: IdmEmulator;
  private readonly evidence: Evidence;
  private readonly missing: (name: string) => never;
  private readonly extra: ExtraNodes;
  private readonly passwordPolicy: PasswordPolicy | undefined;
  private readonly attributePolicies:
    Readonly<Record<string, AttributePolicy>> | undefined;
  constructor(
    idm: IdmEmulator,
    evidence: Evidence,
    missing: (name: string) => never,
    extra: Omit<ExtraContext, "idm" | "evidence" | "missing">,
    passwordPolicy?: PasswordPolicy,
    attributePolicies?: Readonly<Record<string, AttributePolicy>>,
  ) {
    this.passwordPolicy = passwordPolicy;
    this.attributePolicies = attributePolicies;
    this.extra = new ExtraNodes({ idm, evidence, missing, ...extra });
    this.idm = idm;
    this.evidence = evidence;
    this.missing = missing;
  }
  /**
   * The attributes an identity node acts on. With both buckets holding `objectAttributes` (a
   * Validated Password page leaves exactly that) they are overlaid key by key, transient winning:
   * Create, Patch and Identify were each measured so on 2026-11-25 (docs/api/25, "split"), and
   * none of them rewrites either bucket's own map. Only flat values were measured, so a key both
   * buckets hold as objects is missing.
   */
  private attributes(
    shared: Document,
    transient: Document,
  ): Document | undefined {
    const both =
      Object.hasOwn(transient, "objectAttributes") &&
      Object.hasOwn(shared, "objectAttributes");
    const attrs = Object.hasOwn(transient, "objectAttributes")
      ? transient.objectAttributes
      : shared.objectAttributes;
    if (attrs === undefined) return undefined;
    if (!isDocument(attrs)) this.missing("objectAttributes is not an object");
    if (!both) return attrs;
    const base = shared.objectAttributes;
    if (!isDocument(base)) this.missing("objectAttributes is not an object");
    const merged: Document = { ...base };
    for (const [key, value] of Object.entries(attrs)) {
      if (isDocument(value) && isDocument(base[key]))
        this.missing("split objectAttributes sharing a nested object key");
      setOwn(merged, key, value);
    }
    this.evidence.measured(
      "identity nodes read split objectAttributes as the shared map overlaid by the transient one, key by key, and rewrite neither bucket's map",
      "docs/api/25-journey-runtime.md (split objectAttributes, 2026-11-25)",
    );
    return merged;
  }
  /**
   * `ValidatedPasswordNode` with `validateInput: true` (doc 25, "Validated password", 2026-11-25): one
   * `ValidatedCreatePasswordCallback`; a password failing the policy sends it again with the failed
   * requirements, CHARACTER_SET before LENGTH_BASED, as JSON text; a validate-only submission sends
   * it again with `validateOnly` true and writes nothing; an accepted password writes what
   * `validateInput: false` writes.
   */
  private validatedPassword(
    transient: Document,
    submitted: CallbackEffect[],
  ): NodeResult {
    const policy = this.passwordPolicy;
    if (!policy)
      this.missing(
        "ValidatedPasswordNode validateInput: true needs JourneyOptions.passwordPolicy",
      );
    if (Object.hasOwn(transient, "objectAttributes"))
      this.missing(
        "ValidatedPasswordNode over existing transient objectAttributes",
      );
    const callback = (
      failed: string[],
      validateOnly: boolean,
    ): CallbackEffect => ({
      type: "ValidatedCreatePasswordCallback",
      echoOn: false,
      policies: {
        policyRequirements: ["VALID_TYPE"],
        fallbackPolicies: null,
        name: "password",
        policies: [
          {
            policyRequirements: ["VALID_TYPE"],
            policyId: "valid-type",
            params: { types: ["string"] },
          },
        ],
        conditionalPolicies: null,
      },
      failedPolicies: failed,
      validateOnly,
      prompt: "Password",
    });
    this.evidence.measured(
      "ValidatedPasswordNode with validateInput true sends a ValidatedCreatePasswordCallback (echoOn false, VALID_TYPE policies, failedPolicies, validateOnly, prompt Password) with an empty input and a validateOnly input",
      PASSWORD_SOURCE,
    );
    if (!submitted.length)
      return { outcome: null, callbacks: [callback([], false)] };
    const [answer] = submitted;
    if (
      submitted.length !== 1 ||
      answer?.type !== "ValidatedCreatePasswordCallback" ||
      typeof answer.value !== "string" ||
      typeof answer.validateOnly !== "boolean"
    )
      this.missing("ValidatedPasswordNode callback submission");
    const value = answer.value;
    const failed: string[] = [];
    if (policy.characterSets?.length) {
      if (policy.characterSets.some((set) => !set.startsWith("0:")))
        this.missing(
          "ValidatedPasswordNode character set with a non-zero minimum count",
        );
      const minimum = policy.minCharacterSets ?? policy.characterSets.length;
      const met = policy.characterSets.filter((set) =>
        [...set.slice(2)].some((char) => value.includes(char)),
      ).length;
      if (met < minimum)
        failed.push(
          `{ "policyRequirement": "CHARACTER_SET", "params": { "allow-unclassified-characters": true, "character-set-ranges": [  ], "character-sets": [ ${policy.characterSets.map((set) => JSON.stringify(set)).join(", ")} ], "min-character-sets": ${minimum} } }`,
        );
    }
    if (policy.minLength !== undefined && [...value].length < policy.minLength)
      failed.push(
        `{ "policyRequirement": "LENGTH_BASED", "params": { "max-password-length": 0, "min-password-length": ${policy.minLength} } }`,
      );
    if (answer.validateOnly)
      return { outcome: null, callbacks: [callback(failed, true)] };
    if (failed.length)
      return { outcome: null, callbacks: [callback(failed, false)] };
    setOwn(transient, "password", value);
    setOwn(transient, "objectAttributes", { password: value });
    this.evidence.measured(
      "ValidatedPasswordNode writes transient password and objectAttributes {password}",
      PASSWORD_SOURCE,
    );
    return { outcome: "outcome", callbacks: [] };
  }
  /** Shared objectAttributes a node merges into; a non-object one is unmeasured. */
  private sharedAttributes(shared: Document, type: string): Document {
    if (!Object.hasOwn(shared, "objectAttributes")) return {};
    if (!isDocument(shared.objectAttributes))
      this.missing(`${type} over non-object shared objectAttributes`);
    return shared.objectAttributes;
  }
  private async call(
    method: HostCall["method"],
    args: JsonValue[],
  ): Promise<RestResult> {
    const response = await this.idm.hostRequest({
      type: "call",
      id: "journey-node",
      seq: 0,
      method,
      args,
    });
    for (const entry of response.provenance)
      this.evidence.add(entry.kind, entry.behavior, entry.source);
    if (response.emulation) throw new EmulationFailure(response.emulation);
    return response;
  }
  private async find(
    resource: string,
    attribute: string,
    lookup: JsonValue,
  ): Promise<Document | undefined> {
    if (typeof lookup !== "string")
      this.missing("identity lookup with non-string value");
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(attribute))
      this.missing(`identity lookup attribute: ${attribute}`);
    const result = await this.call("query", [
      resource,
      { _queryFilter: `${attribute} eq ${JSON.stringify(lookup)}` },
    ]);
    if (result.status >= 400)
      this.missing(`identity lookup error: ${result.status}`);
    if (!isDocument(result.body) || !Array.isArray(result.body.result))
      this.missing("identity lookup result shape");
    if (result.body.result.length > 1)
      this.missing("identity lookup matched multiple records");
    const first = result.body.result[0];
    return isDocument(first) ? first : undefined;
  }
  async run(
    type: string,
    config: NodeDocument,
    shared: Document,
    transient: Document,
    submitted: CallbackEffect[],
    defaultResource: string,
    id: string,
    cookies?: Readonly<Record<string, string>>,
  ): Promise<NodeResult> {
    if (EXTRA_TYPES.includes(type))
      return this.extra.run(
        type,
        id,
        config,
        shared,
        transient,
        submitted,
        defaultResource,
        cookies,
      );
    const done = (outcome: string): NodeResult => ({ outcome, callbacks: [] });
    if (
      [
        "UsernameCollectorNode",
        "PasswordCollectorNode",
        "ValidatedPasswordNode",
      ].includes(type)
    ) {
      if (
        type === "ValidatedPasswordNode" &&
        typeof config.validateInput !== "boolean"
      )
        this.missing("ValidatedPasswordNode validateInput unspecified");
      const validated =
        type === "ValidatedPasswordNode" && config.validateInput === true;
      // Measured only with passwordAttribute "password" (scripts/experiment-journey-collectors.sh).
      if (
        type === "ValidatedPasswordNode" &&
        config.passwordAttribute !== "password"
      )
        this.missing(
          `ValidatedPasswordNode passwordAttribute: ${typeof config.passwordAttribute === "string" ? config.passwordAttribute : "unspecified"}`,
        );
      const username = type === "UsernameCollectorNode";
      if (validated) return this.validatedPassword(transient, submitted);
      if (!submitted.length) {
        this.evidence.assumed(
          "UsernameCollectorNode prompt defaults to User Name; PasswordCollectorNode prompt defaults to Password",
        );
        return {
          outcome: null,
          callbacks: [
            {
              type: username ? "NameCallback" : "PasswordCallback",
              prompt: username ? "User Name" : "Password",
            },
          ],
        };
      }
      if (
        submitted.length !== 1 ||
        submitted[0]?.type !==
          (username ? "NameCallback" : "PasswordCallback") ||
        typeof submitted[0].value !== "string"
      )
        this.missing(`${type} callback submission`);
      const answer = submitted[0].value;
      // Measured 2026-11-25: an empty answer re-sends the callback (alone or in a Page),
      // writing nothing; a whitespace-only answer is accepted. ValidatedPasswordNode is unmeasured.
      if (answer === "" && type !== "ValidatedPasswordNode") {
        this.evidence.measured(
          "UsernameCollectorNode and PasswordCollectorNode re-send their callback on an empty answer and write nothing",
          "docs/api/25-journey-runtime.md (built-in node types, 2026-11-25)",
        );
        return {
          outcome: null,
          callbacks: [
            {
              type: username ? "NameCallback" : "PasswordCallback",
              prompt: username ? "User Name" : "Password",
            },
          ],
        };
      }
      // Doc 25 measured the write over no transient objectAttributes only; over
      // existing keys, replace versus merge is unmeasured.
      if (
        type === "ValidatedPasswordNode" &&
        Object.hasOwn(transient, "objectAttributes")
      )
        this.missing(
          "ValidatedPasswordNode over existing transient objectAttributes",
        );
      setOwn(
        username ? shared : transient,
        username ? "username" : "password",
        answer,
      );
      if (type === "ValidatedPasswordNode")
        setOwn(transient, "objectAttributes", { password: answer });
      this.evidence.measured(
        "collectors write shared username and transient password; ValidatedPasswordNode writes transient objectAttributes without merging",
        SOURCE,
      );
      return done("outcome");
    }
    if (type === "AttributeCollectorNode") {
      const validate = config.validateInputs !== false;
      if (config.validateInputs !== false && config.validateInputs !== true)
        this.missing("AttributeCollectorNode validateInputs configuration");
      if (validate && !this.attributePolicies)
        this.missing(
          "AttributeCollectorNode validateInputs: true needs JourneyOptions.attributePolicies",
        );
      if (config.required !== undefined && typeof config.required !== "boolean")
        this.missing("AttributeCollectorNode required configuration");
      if (
        !Array.isArray(config.attributesToCollect) ||
        !config.attributesToCollect.length ||
        config.attributesToCollect.some((key) => typeof key !== "string")
      )
        this.missing(
          "AttributeCollectorNode attributesToCollect configuration",
        );
      const resource =
        typeof config.identityResource === "string"
          ? config.identityResource
          : defaultResource;
      if (!resource.startsWith("managed/"))
        this.missing(`AttributeCollectorNode schema: ${resource}`);
      const schema = this.idm.config.managedType(resource.slice(8))?.schema;
      if (!schema) this.missing(`AttributeCollectorNode schema: ${resource}`);
      const attrs: Document = {},
        callbacks: CallbackEffect[] = [];
      let invalid = false;
      if (
        submitted.length &&
        submitted.length !== config.attributesToCollect.length
      )
        this.missing("AttributeCollectorNode callback count");
      for (const [index, attribute] of (
        config.attributesToCollect as string[]
      ).entries()) {
        const path = attribute.split("/");
        let property: PropertyConfig | undefined,
          level: unknown = schema.properties;
        for (const [depth, segment] of path.entries()) {
          property =
            isDocument(level) && Object.hasOwn(level, segment)
              ? (level[segment] as PropertyConfig)
              : undefined;
          if (depth < path.length - 1) {
            if (property?.type !== "object")
              this.missing(`AttributeCollectorNode schema: ${attribute}`);
            level = property.properties;
          }
        }
        if (
          !property ||
          property.type !== "string" ||
          typeof property.title !== "string"
        )
          this.missing(
            `AttributeCollectorNode string schema title: ${attribute}`,
          );
        if (path.length > 1)
          this.evidence.assumed(
            "AttributeCollectorNode treats a slash-path string attribute like a flat one; the path walk and nested storage were measured with a boolean property",
          );
        const cb = submitted[index];
        if (
          cb &&
          (cb.type !== "StringAttributeInputCallback" ||
            cb.name !== attribute ||
            typeof cb.value !== "string")
        )
          this.missing("AttributeCollectorNode callback submission");
        if (cb?.validateOnly === true && !validate)
          this.missing("AttributeCollectorNode validateOnly");
        const answer = typeof cb?.value === "string" ? cb.value : "";
        let policies: Document = {};
        const failedPolicies: string[] = [];
        if (validate) {
          const policy = this.attributePolicies?.[attribute];
          if (!policy)
            this.missing(
              `AttributeCollectorNode validateInputs: true needs JourneyOptions.attributePolicies for ${attribute}`,
            );
          const unmodelled = policy.requirements.filter(
            (r) => r !== "REQUIRED" && r !== "VALID_EMAIL_ADDRESS_FORMAT",
          );
          if (unmodelled.length)
            this.missing(
              `AttributeCollectorNode policy requirement: ${unmodelled.join(", ")}`,
            );
          policies = { ...(policy.policies as Document) };
          if (cb) {
            // Measured: an empty answer fails REQUIRED only when the node is not itself `required`
            // (which retries with no failed policy), and is not also checked for an address format.
            if (answer === "") {
              if (
                config.required !== true &&
                policy.requirements.includes("REQUIRED")
              )
                failedPolicies.push('{ "policyRequirement": "REQUIRED" }');
            } else if (
              policy.requirements.includes("VALID_EMAIL_ADDRESS_FORMAT") &&
              !looksLikeEmail(answer)
            )
              failedPolicies.push(
                '{ "policyRequirement": "VALID_EMAIL_ADDRESS_FORMAT" }',
              );
          }
          if (cb?.validateOnly === true && !failedPolicies.length)
            this.missing(
              "AttributeCollectorNode validateOnly with nothing failing",
            );
          if (failedPolicies.length) invalid = true;
        }
        setNested(attrs, path, answer);
        if (config.required === true && answer === "") invalid = true;
        else if (cb && answer === "")
          this.evidence.measured(
            "AttributeCollectorNode writes a blank non-required answer as the empty string",
            "docs/api/25-journey-runtime.md (Attribute Collector, a non-required attribute left blank, 2026-11-04)",
          );
        callbacks.push({
          type: "StringAttributeInputCallback",
          name: attribute,
          prompt: property.title,
          required: config.required === true,
          policies,
          failedPolicies,
          validateOnly: cb?.validateOnly === true,
          value: answer,
        });
      }
      if (validate)
        this.evidence.measured(
          "AttributeCollectorNode validateInputs true sends the attribute's policies, retries a failing page with the answers kept and failedPolicies as JSON text, and a validate-only submission is sent again with validateOnly true",
          VALIDATE_SOURCE,
        );
      this.evidence.measured(
        "AttributeCollectorNode callback fields, shared objectAttributes and required-empty retry with empty failedPolicies",
        SOURCE,
      );
      if (!submitted.length || invalid) return { outcome: null, callbacks };
      const previous = this.sharedAttributes(shared, type);
      // Measured: the collected keys come out in alphabetical order whatever the collection order.
      const ordered: Document = {};
      for (const key of Object.keys(attrs).sort())
        setOwn(ordered, key, attrs[key] as JsonValue);
      this.evidence.measured(
        "AttributeCollectorNode stores a slash path as nested objectAttributes and orders the collected top-level keys alphabetically",
        "docs/api/25-journey-runtime.md (Attribute Collector slash paths, 2026-11-25)",
      );
      setOwn(shared, "objectAttributes", { ...previous, ...ordered });
      this.evidence.assumed(
        "AttributeCollectorNode merges collected attributes into existing shared objectAttributes",
      );
      return done("outcome");
    }
    const attrs = this.attributes(shared, transient);
    if (config.identityResource === undefined)
      this.evidence.assumed(
        "identity nodes default identityResource to the executing tree identityResource",
      );
    const resource =
      typeof config.identityResource === "string"
        ? config.identityResource
        : defaultResource;
    if (type === "CreateObjectNode") {
      if (!attrs) this.missing("CreateObjectNode missing objectAttributes");
      const result = await this.call("create", [resource, null, attrs]);
      this.evidence.measured(
        "CreateObjectNode creates through IDM and writes no state; missing-required rejection yields FAILURE",
        SOURCE,
      );
      if (result.status >= 400) {
        if (![400, 403].includes(result.status))
          this.missing(`CreateObjectNode IDM error: ${result.status}`);
        this.evidence.assumed(
          "CreateObjectNode maps IDM 400/403 rejections to FAILURE; missing required is measured",
        );
      }
      return done(result.status < 400 ? "CREATED" : "FAILURE");
    }
    if (!["IdentifyExistingUserNode", "PatchObjectNode"].includes(type))
      this.missing(`node type: ${type}`);
    if (type === "PatchObjectNode") {
      if (config.patchAsObject !== false)
        this.missing("PatchObjectNode patchAsObject: true or unspecified");
      if (config.ignoredFields !== undefined) {
        if (!Array.isArray(config.ignoredFields))
          this.missing("PatchObjectNode ignoredFields that is not an array");
        // Doc 25 (2026-11-25): an entry is an attribute name, with or without a leading slash.
        // A nested pointer was not measured.
        for (const entry of config.ignoredFields)
          if (
            typeof entry !== "string" ||
            entry.replace(/^\//, "").includes("/")
          )
            this.missing(
              "PatchObjectNode ignoredFields entry that is not a plain attribute name",
            );
      }
    }
    if (type === "IdentifyExistingUserNode") {
      if (typeof config.identifier !== "string")
        this.missing("IdentifyExistingUserNode identifier configuration");
    }
    if (typeof config.identityAttribute !== "string")
      this.missing(`${type} identityAttribute configuration`);
    const objectValue = attrs
      ? own(attrs, config.identityAttribute)
      : undefined;
    const username = Object.hasOwn(transient, "username")
      ? transient.username
      : shared.username;
    let lookup: JsonValue | undefined;
    if (type === "IdentifyExistingUserNode") {
      // Doc 25 (2026-11-04): objectAttributes[identityAttribute], when present and not null, is
      // the only lookup value — "" included; username is read only when that value is absent or
      // null, and a miss never falls back.
      const present = objectValue !== undefined && objectValue !== null;
      lookup = present ? objectValue : username;
      if (lookup === undefined || lookup === null) {
        // Doc 25 (2026-11-25): no usable identity value is a miss, never an error; nothing is
        // written. A null username with no usable objectAttributes value is the same.
        this.evidence.measured(
          "IdentifyExistingUserNode with no identity value (no username and no objectAttributes[identityAttribute], or only nulls) takes false and writes no state",
          EMPTY_SOURCE,
        );
        return done("false");
      }
      if (present && username !== undefined)
        this.evidence.measured(
          "IdentifyExistingUserNode looks up objectAttributes[identityAttribute] alone when present, ignoring username",
          PRECEDENCE_SOURCE,
        );
      if (objectValue === "")
        this.evidence.measured(
          "IdentifyExistingUserNode looks up an empty objectAttributes[identityAttribute] and does not fall back",
          BLANK_SOURCE,
        );
      if (objectValue === null)
        this.evidence.measured(
          "IdentifyExistingUserNode treats a null objectAttributes[identityAttribute] as absent and uses username",
          BLANK_SOURCE,
        );
    } else {
      // Patch Object precedence between the two sources is not measured.
      if (
        objectValue !== undefined &&
        username !== undefined &&
        objectValue !== username
      )
        this.missing(`${type} conflicting identity sources`);
      if (objectValue === null || username === null)
        this.missing(`${type} null identity source`);
      lookup = objectValue ?? username;
    }
    if (lookup === undefined) {
      // Patch (doc 25: shared _id plus objectAttributes {sn} is FAILURE). Identify returned above.
      if (!attrs) this.missing("PatchObjectNode missing objectAttributes");
      this.evidence.measured(
        "PatchObjectNode ignores _id, uses identity attribute or username, and aborts on no match",
        SOURCE,
      );
      return done("FAILURE");
    }
    const match = await this.find(resource, config.identityAttribute, lookup);
    if (type === "IdentifyExistingUserNode") {
      this.evidence.measured(
        "IdentifyExistingUserNode lookup and shared _id/username/identifier writeback",
        SOURCE,
      );
      if (!match) return done("false");
      const key = config.identifier as string;
      if (typeof own(match, key) !== "string" || typeof match._id !== "string")
        this.missing("IdentifyExistingUserNode identifier result");
      const identifier = own(match, key) as string;
      setOwn(shared, "_id", match._id);
      setOwn(shared, "username", identifier);
      setOwn(shared, "objectAttributes", {
        ...this.sharedAttributes(shared, type),
        [key]: identifier,
      });
      return done("true");
    }
    this.evidence.measured(
      "PatchObjectNode ignores _id, uses identity attribute or username, and aborts on no match",
      SOURCE,
    );
    if (!match)
      return {
        outcome: null,
        callbacks: [],
        abort: {
          status: 401,
          body: { code: 401, reason: "Unauthorized", message: "Login failure" },
        },
      };
    if (!attrs) this.missing("PatchObjectNode missing objectAttributes");
    // Doc 25 (2026-11-25): a field named in ignoredFields ("sn" or "/sn", matched exactly and
    // case-sensitively) is dropped from the patch, whichever objectAttributes map held it.
    const ignored = new Set(
      ((config.ignoredFields as string[] | undefined) ?? []).map((name) =>
        name.replace(/^\//, ""),
      ),
    );
    if (ignored.size)
      this.evidence.measured(
        "PatchObjectNode ignoredFields drops each named attribute (with or without a leading slash, case-sensitive, from either objectAttributes map) from the patch; the lookup is unaffected, and ignoring every field still takes PATCHED",
        IGNORED_SOURCE,
      );
    const patches = Object.entries(attrs)
      .filter(([key]) => !ignored.has(key))
      .map(([key, data]) => ({
        operation: "replace",
        field: `/${key.replace(/~/g, "~0").replace(/\//g, "~1")}`,
        value: data,
      }));
    if (!patches.length) {
      this.evidence.assumed(
        "PatchObjectNode with every field ignored sends no patch (the outcome PATCHED and the unchanged record are measured; whether IDM saw an empty patch is not)",
      );
      return done("PATCHED");
    }
    const result = await this.call("patch", [
      `${resource}/${String(match._id)}`,
      null,
      patches,
    ]);
    if (result.status >= 400)
      this.missing(`PatchObjectNode IDM error: ${result.status}`);
    return done("PATCHED");
  }
}
