/** Pieces more than one journey node needs, kept here so each exists once. */
import { EmulationFailure } from "../emulation.ts";
import { isDocument } from "../idm/document.ts";
import type { IdmEmulator } from "../idm/emulator.ts";
import type { Evidence } from "../idm/provenance.ts";
import type { Document, RestResult } from "../idm/types.ts";
import type { HostCall, JsonValue } from "../protocol.ts";

/** AM's answer when a node fails the journey outright. */
export const LOGIN_FAILURE = {
  status: 401,
  body: { code: 401, reason: "Unauthorized", message: "Login failure" },
} as const;
/** A fresh copy of {@link LOGIN_FAILURE}, for a result the caller may keep. */
export const loginFailure = (): {
  status: number;
  body: { code: number; reason: string; message: string };
} => structuredClone(LOGIN_FAILURE) as never;

/**
 * Session properties AM owns: SetSessionPropertiesNode's validation lists them (doc 25, measured
 * 2026-11-25), and a script's putSessionProperty of the first four was measured to fail.
 */
export const SYSTEM_SESSION_PROPERTIES: readonly string[] = [
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

/**
 * One IDM call from a journey node, with its evidence carried over and an emulation failure
 * rethrown. `journey` is the AM view of alpha_user, which omits `password`; Data Store Decision
 * and the persistent cookie read through the internal view.
 */
export async function idmCall(
  idm: IdmEmulator,
  evidence: Evidence,
  method: HostCall["method"],
  args: JsonValue[],
  journey: boolean,
): Promise<RestResult> {
  const response = await idm.hostRequest({ type: "call", id: "journey-node", seq: 0, method, args }, journey);
  for (const entry of response.provenance) evidence.add(entry.kind, entry.behavior, entry.source);
  if (response.emulation) throw new EmulationFailure(response.emulation);
  return response;
}

/** The records matching `attribute eq value`; an IDM error or a malformed result is missing. */
export async function lookupIdentities(
  idm: IdmEmulator,
  evidence: Evidence,
  missing: (name: string) => never,
  query: { resource: string; attribute: string; value: string; fields?: string; journey: boolean },
): Promise<Document[]> {
  const params: Document = { _queryFilter: `${query.attribute} eq ${JSON.stringify(query.value)}` };
  if (query.fields !== undefined) params._fields = query.fields;
  const result = await idmCall(idm, evidence, "query", [query.resource, params], query.journey);
  if (result.status >= 400) missing(`identity lookup error: ${result.status}`);
  if (!isDocument(result.body) || !Array.isArray(result.body.result))
    missing("identity lookup result shape");
  return result.body.result.filter(isDocument);
}

/**
 * The text a node shows from a localized map (`{locale: text}`), by the rule measured for a
 * Page's header (doc 25, 2026-11-25): the `en` entry, else the only entry. Several locales with
 * no `en`, or several under an Accept-Language request, are missing: which one AM picks is
 * unmeasured. Returns undefined for an empty map.
 */
export function localizedText(
  map: Document,
  what: string,
  missing: (name: string) => never,
  acceptLanguage: boolean,
): string | undefined {
  const keys = Object.keys(map);
  if (!keys.length) return undefined;
  if (acceptLanguage && keys.length > 1) missing(`${what} with several locales and Accept-Language`);
  const key = Object.hasOwn(map, "en") ? "en" : keys.length === 1 ? keys[0] : undefined;
  if (key === undefined) missing(`${what} with several locales and no en`);
  const value = map[key];
  if (typeof value !== "string") missing(`${what} value`);
  return value;
}

/**
 * A submitted callback index. A string is read as a number, but a blank one is refused rather
 * than read as 0 (Number("") is 0): what AM does with an empty answer is unmeasured.
 */
export function answerIndex(
  value: unknown,
  what: string,
  missing: (name: string) => never,
): unknown {
  if (typeof value !== "string") return value;
  if (value.trim() === "") missing(`${what} answer: ${JSON.stringify(value)}`);
  return Number(value);
}
