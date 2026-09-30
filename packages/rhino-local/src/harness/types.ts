import type { z } from "zod";
import type { Expect, Given, HttpStub, JsonObject, JsonValue, OpenidmFailureStub } from "../case/types.ts";

/** A header or parameter value. An array is sent as repeated occurrences. */
export type WireValue = string | readonly string[];
export type WireMap = Readonly<Record<string, WireValue>>;

export interface StateChannels {
  shared?: JsonObject;
  transient?: JsonObject;
}

/**
 * Everything a run can carry into the script, in one shape. The same
 * channels are declarable on the suite (`always`), overridable per test, and
 * reachable from `beforeRun` — so there is one place to look, whichever
 * surface you are reading.
 */
export interface Channels {
  state?: StateChannels;
  /**
   * Declared `systemEnv.getProperty("esv.<name>")` values, keyed by `<name>`.
   * Full `esv.<name>` properties and `esv-...` management IDs are rejected.
   * AIC reads its own tenant ESVs; the harness never changes tenant ESVs.
   */
  esv?: Readonly<Record<string, string | null>>;
  /** Also seed declared ESVs under `esv.<name>` in shared state on both lanes. */
  esvInState?: boolean;
  /** Sent on the authenticate request. No script can assign these bindings. */
  headers?: WireMap;
  /** Sent on the authenticate request. */
  params?: WireMap;
  /** Cookie name/value pairs sent on the authenticate request. */
  cookies?: Readonly<Record<string, string>>;
  /** Local `cookieName` binding; AIC uses the tenant's serverinfo value. */
  cookieName?: string;
  /** Ordered local HTTP replies; AIC cannot inject them. */
  http?: readonly HttpStub[];
  /** Fail one numbered `openidm` call locally. AIC cannot inject failures. */
  openidmFailures?: readonly OpenidmFailureStub[];
  /** Local binding replacements as JavaScript expressions. AIC cannot inject these. */
  bindingOverrides?: Readonly<Record<string, string>>;
  /**
   * `existingSession` — session properties the script sees, as a flat string
   * map. Declaring it at all (`session: {}` included) asks for a logged-in
   * session; the AIC lane pays for it with an extra round trip, because a
   * session only exists once a journey has run to completion, so the lane
   * runs a two-line mini journey and forwards its cookie to the subject.
   *
   * Custom properties only. The ones AM sets itself — `UserId`, `AuthLevel`
   * and the rest — are refused, because `putSessionProperty` cannot override
   * them and trying fails the whole login with an unexplained 401. The five
   * AM derives from the principal come from `state.username` instead.
   *
   * Nothing about the subject tree changes, and the principal need not exist
   * as a managed object (both measured 2026-09-14).
   */
  session?: JsonObject;
}

/** The mutable draft `beforeRun` is handed. */
export interface RequestDraft {
  state: { shared: JsonObject; transient: JsonObject };
  esv: Record<string, string | null>;
  esvInState: boolean;
  headers: Record<string, string[]>;
  params: Record<string, string[]>;
  cookies: Record<string, string>;
  cookieName?: string;
  http: HttpStub[];
  openidmFailures: OpenidmFailureStub[];
  bindingOverrides: Record<string, string>;
  session: JsonObject;
  /** Whether either level asked for a session at all. See mergeChannels. */
  sessionRequested: boolean;
}

/** A managed record the harness creates and is therefore responsible for. */
export interface FixtureSpec {
  type: string;
  record: JsonObject;
}

export interface BeforeRunContext<TInput> {
  input: TInput;
  request: RequestDraft;
  fixtures: FixtureCreator;
}

export interface FixtureCreator {
  create(type: string, record: JsonObject | JsonObject[]): Promise<void>;
}

/**
 * What checks and cleanup receive on both lanes. The surface is shared, but
 * record shape is not projected: AIC reads retain tenant-materialized fields.
 */
export interface IdmHandle {
  read(resource: string): Promise<JsonObject | null>;
  query(type: string, filter: Readonly<Record<string, JsonValue>>): Promise<JsonObject[]>;
  delete(resource: string): Promise<void>;
}

export interface CleanupContext<TInput> {
  input: TInput;
}

export interface SuiteSpec<TSchema extends z.ZodType> {
  name: string;
  /** Local-only binding name; defaults to `name`. AIC uses its uploaded name. */
  scriptName?: string;
  /** Author source, already loaded. */
  script: string;
  /** The outcome vocabulary. Required here — a lease has to declare it. */
  outcomes: readonly string[];
  /** AM LIBRARY source keyed by the name passed to `require()`. */
  libraries?: Readonly<Record<string, string>>;
  inputs?: TSchema;
  always?: Channels;
  fixtures?: Readonly<Record<string, FixtureSpec>>;
  beforeRun?: (ctx: BeforeRunContext<z.output<TSchema>>) => void | Promise<void>;
  cleanup?: (idm: IdmHandle, ctx: CleanupContext<z.output<TSchema>>) => Promise<void>;
}

/** Per-test overrides, merged over the suite's `always`. */
export interface RunOverrides extends Channels {
  expect: Expect;
}

export type { Expect, Given, JsonObject, JsonValue };
