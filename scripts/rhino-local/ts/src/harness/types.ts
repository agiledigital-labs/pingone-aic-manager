import type { z } from "zod";
import type { Expect, Given, JsonObject, JsonValue } from "../case/types.ts";

/** A header or parameter value. An array is sent as repeated occurrences. */
export type WireValue = string | readonly string[];
export type WireMap = Readonly<Record<string, WireValue>>;

export interface StateChannels {
  shared?: JsonObject;
  transient?: JsonObject;
}

/**
 * Everything a run can carry into the script, in one shape. The same five
 * channels are declarable on the suite (`always`), overridable per test, and
 * reachable from `beforeRun` — so there is one place to look, whichever
 * surface you are reading.
 */
export interface Channels {
  state?: StateChannels;
  /**
   * ESV overrides. NOT the `systemEnv` binding — these compile to shared
   * state under `esv.<name>`, which the tenant's config library consults
   * before falling back to the real ESV. Seed the real binding with
   * `given.esv` instead; both mechanisms exist and mean different things.
   */
  esv?: Readonly<Record<string, string>>;
  /** Sent on the authenticate request. No script can assign these bindings. */
  headers?: WireMap;
  /** Sent on the authenticate request. */
  params?: WireMap;
  /**
   * `existingSession`. Declared for symmetry and currently inert: nobody has
   * observed a populated one on either lane, so a mock would be an invention
   * that both lanes would agree on while both ran the guess. Setting it is
   * refused rather than silently ignored.
   */
  session?: JsonObject;
}

/** The mutable draft `beforeRun` is handed. */
export interface RequestDraft {
  state: { shared: JsonObject; transient: JsonObject };
  esv: Record<string, string>;
  headers: Record<string, string[]>;
  params: Record<string, string[]>;
  session: JsonObject;
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

/** What `cleanup` is handed: the same surface on both lanes, by design. */
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
  /** Author source, already loaded. */
  script: string;
  /** The outcome vocabulary. Required here — a lease has to declare it. */
  outcomes: readonly string[];
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
