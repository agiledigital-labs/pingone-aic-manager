/** Collector and identity nodes; managed policy and writes stay in the shared IDM core. */
import type { CallbackEffect } from "../case/types.ts";
import { EmulationFailure } from "../emulation.ts";
import { isDocument, own, setOwn } from "../idm/document.ts";
import type { IdmEmulator } from "../idm/emulator.ts";
import type { Evidence } from "../idm/provenance.ts";
import type { Document, RestResult } from "../idm/types.ts";
import type { HostCall, JsonValue } from "../protocol.ts";
import type { AuthenticateResult, NodeDocument } from "./types.ts";
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
];
export type NodeResult = {
  outcome: string | null;
  callbacks: CallbackEffect[];
  abort?: Omit<AuthenticateResult, "provenance">;
};
const SOURCE = "docs/api/25-journey-runtime.md (measurement agent, 2026-11-02)";
const PRECEDENCE_SOURCE =
  "docs/api/25-journey-runtime.md (Identify Existing User precedence, 2026-11-04)";
const BLANK_SOURCE =
  "docs/api/25-journey-runtime.md (A blank or null lookup value, and letter case, 2026-11-04)";
export class JourneyNodes {
  private readonly idm: IdmEmulator;
  private readonly evidence: Evidence;
  private readonly missing: (name: string) => never;
  constructor(idm: IdmEmulator, evidence: Evidence, missing: (name: string) => never) {
    this.idm = idm;
    this.evidence = evidence;
    this.missing = missing;
  }
  private attributes(shared: Document, transient: Document): Document | undefined {
    const attrs = Object.hasOwn(transient, "objectAttributes")
      ? transient.objectAttributes
      : shared.objectAttributes;
    if (attrs === undefined) return undefined;
    if (!isDocument(attrs)) this.missing("objectAttributes is not an object");
    return attrs;
  }
  /** Shared objectAttributes a node merges into; a non-object one is unmeasured. */
  private sharedAttributes(shared: Document, type: string): Document {
    if (!Object.hasOwn(shared, "objectAttributes")) return {};
    if (!isDocument(shared.objectAttributes))
      this.missing(`${type} over non-object shared objectAttributes`);
    return shared.objectAttributes;
  }
  private async call(method: HostCall["method"], args: JsonValue[]): Promise<RestResult> {
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
    if (typeof lookup !== "string") this.missing("identity lookup with non-string value");
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(attribute))
      this.missing(`identity lookup attribute: ${attribute}`);
    const result = await this.call("query", [
      resource,
      { _queryFilter: `${attribute} eq ${JSON.stringify(lookup)}` },
    ]);
    if (result.status >= 400) this.missing(`identity lookup error: ${result.status}`);
    if (!isDocument(result.body) || !Array.isArray(result.body.result))
      this.missing("identity lookup result shape");
    if (result.body.result.length > 1) this.missing("identity lookup matched multiple records");
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
  ): Promise<NodeResult> {
    const done = (outcome: string): NodeResult => ({ outcome, callbacks: [] });
    if (
      ["UsernameCollectorNode", "PasswordCollectorNode", "ValidatedPasswordNode"].includes(type)
    ) {
      if (type === "ValidatedPasswordNode" && config.validateInput !== false)
        this.missing("ValidatedPasswordNode validateInput: true or unspecified");
      // Measured only with passwordAttribute "password" (scripts/experiment-journey-collectors.sh).
      if (type === "ValidatedPasswordNode" && config.passwordAttribute !== "password")
        this.missing(
          `ValidatedPasswordNode passwordAttribute: ${typeof config.passwordAttribute === "string" ? config.passwordAttribute : "unspecified"}`,
        );
      const username = type === "UsernameCollectorNode";
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
        submitted[0]?.type !== (username ? "NameCallback" : "PasswordCallback") ||
        typeof submitted[0].value !== "string"
      )
        this.missing(`${type} callback submission`);
      const answer = submitted[0].value;
      // Doc 25 measured the write over no transient objectAttributes only; over
      // existing keys, replace versus merge is unmeasured.
      if (type === "ValidatedPasswordNode" && Object.hasOwn(transient, "objectAttributes"))
        this.missing("ValidatedPasswordNode over existing transient objectAttributes");
      setOwn(username ? shared : transient, username ? "username" : "password", answer);
      if (type === "ValidatedPasswordNode")
        setOwn(transient, "objectAttributes", { password: answer });
      this.evidence.measured(
        "collectors write shared username and transient password; ValidatedPasswordNode writes transient objectAttributes without merging",
        SOURCE,
      );
      return done("outcome");
    }
    if (type === "AttributeCollectorNode") {
      if (config.validateInputs !== false)
        this.missing(
          "AttributeCollectorNode policy evaluation; supply the IDM policy seam for managed writes",
        );
      if (config.required !== undefined && typeof config.required !== "boolean")
        this.missing("AttributeCollectorNode required configuration");
      if (
        !Array.isArray(config.attributesToCollect) ||
        !config.attributesToCollect.length ||
        config.attributesToCollect.some((key) => typeof key !== "string")
      )
        this.missing("AttributeCollectorNode attributesToCollect configuration");
      const resource =
        typeof config.identityResource === "string" ? config.identityResource : defaultResource;
      if (!resource.startsWith("managed/"))
        this.missing(`AttributeCollectorNode schema: ${resource}`);
      const schema = this.idm.config.managedType(resource.slice(8))?.schema;
      if (!schema) this.missing(`AttributeCollectorNode schema: ${resource}`);
      const attrs: Document = {},
        callbacks: CallbackEffect[] = [];
      let invalid = false;
      if (submitted.length && submitted.length !== config.attributesToCollect.length)
        this.missing("AttributeCollectorNode callback count");
      for (const [index, attribute] of (config.attributesToCollect as string[]).entries()) {
        const property =
          schema.properties && Object.hasOwn(schema.properties, attribute)
            ? schema.properties[attribute]
            : undefined;
        if (!property || property.type !== "string" || typeof property.title !== "string")
          this.missing(`AttributeCollectorNode string schema title: ${attribute}`);
        const cb = submitted[index];
        if (
          cb &&
          (cb.type !== "StringAttributeInputCallback" ||
            cb.name !== attribute ||
            typeof cb.value !== "string")
        )
          this.missing("AttributeCollectorNode callback submission");
        if (cb?.validateOnly === true) this.missing("AttributeCollectorNode validateOnly");
        const answer = cb?.value ?? "";
        setOwn(attrs, attribute, answer);
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
          policies: {},
          failedPolicies: [],
          validateOnly: false,
          value: answer,
        });
      }
      this.evidence.measured(
        "AttributeCollectorNode callback fields, shared objectAttributes and required-empty retry with empty failedPolicies",
        SOURCE,
      );
      if (!submitted.length || invalid) return { outcome: null, callbacks };
      const previous = this.sharedAttributes(shared, type);
      setOwn(shared, "objectAttributes", { ...previous, ...attrs });
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
      typeof config.identityResource === "string" ? config.identityResource : defaultResource;
    if (type === "CreateObjectNode") {
      if (Object.hasOwn(shared, "objectAttributes") && Object.hasOwn(transient, "objectAttributes"))
        this.missing("CreateObjectNode split objectAttributes across shared and transient");
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
    // Doc 25 measured every identity node with objectAttributes in one bucket only.
    if (Object.hasOwn(shared, "objectAttributes") && Object.hasOwn(transient, "objectAttributes"))
      this.missing(`${type} split objectAttributes across shared and transient`);
    if (type === "PatchObjectNode") {
      if (config.patchAsObject !== false)
        this.missing("PatchObjectNode patchAsObject: true or unspecified");
      if (
        config.ignoredFields !== undefined &&
        (!Array.isArray(config.ignoredFields) || config.ignoredFields.length)
      )
        this.missing("PatchObjectNode ignoredFields");
    }
    if (type === "IdentifyExistingUserNode") {
      if (typeof config.identifier !== "string")
        this.missing("IdentifyExistingUserNode identifier configuration");
    }
    if (typeof config.identityAttribute !== "string")
      this.missing(`${type} identityAttribute configuration`);
    const objectValue = attrs ? own(attrs, config.identityAttribute) : undefined;
    const username = Object.hasOwn(transient, "username") ? transient.username : shared.username;
    let lookup: JsonValue | undefined;
    if (type === "IdentifyExistingUserNode") {
      // Doc 25 (2026-11-04): objectAttributes[identityAttribute], when present and not null, is
      // the only lookup value — "" included; username is read only when that value is absent or
      // null, and a miss never falls back.
      const present = objectValue !== undefined && objectValue !== null;
      lookup = present ? objectValue : username;
      if (lookup === null) this.missing(`${type} null identity source`);
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
      if (objectValue !== undefined && username !== undefined && objectValue !== username)
        this.missing(`${type} conflicting identity sources`);
      if (objectValue === null || username === null) this.missing(`${type} null identity source`);
      lookup = objectValue ?? username;
    }
    if (lookup === undefined) {
      // Measured for Patch only (doc 25: shared _id plus objectAttributes {sn} is FAILURE).
      if (type === "IdentifyExistingUserNode")
        this.missing("IdentifyExistingUserNode with no identity value");
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
    const patches = Object.entries(attrs).map(([key, data]) => ({
      operation: "replace",
      field: `/${key.replace(/~/g, "~0").replace(/\//g, "~1")}`,
      value: data,
    }));
    const result = await this.call("patch", [`${resource}/${String(match._id)}`, null, patches]);
    if (result.status >= 400) this.missing(`PatchObjectNode IDM error: ${result.status}`);
    return done("PATCHED");
  }
}
