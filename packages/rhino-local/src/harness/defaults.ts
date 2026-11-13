/**
 * Project-wide suite defaults (IMPROVEMENTS #12): channels and libraries a
 * whole project shares, such as the ESVs a common library reads on most
 * paths, declared once instead of in every suite.
 *
 * Precedence, lowest first: the project defaults, the suite's `always`, then a
 * case's own `.run()` overrides — the same per-key merge `mergeChannels`
 * applies between `always` and a case. A suite opts out with
 * `defaults: false`. Defaults only add declarations: nothing here relaxes
 * undeclared-fixture checking, which stays at the suite's `esvUndeclared`
 * (`"error"` unless a layer sets it).
 */
import type { z } from "zod";
import { defineSuite, type Suite } from "./lease.ts";
import type { Channels, SuiteSpec } from "./types.ts";

export interface SuiteDefaults {
  always?: Channels;
  /** AM LIBRARY sources keyed by `require()` name; a suite's own entry wins. */
  libraries?: Readonly<Record<string, string>>;
}

type Combine<K extends keyof Channels> = (base: Channels[K], top: Channels[K]) => Channels[K];
const map = <T extends object>(base: T | undefined, top: T | undefined): T | undefined =>
  base === undefined && top === undefined ? undefined : ({ ...(base ?? {}), ...(top ?? {}) } as T);
// Stub lists: the upper layer's entries first, as mergeChannels orders a case's before the suite's.
const list = <T>(base: readonly T[] | undefined, top: readonly T[] | undefined): readonly T[] | undefined =>
  base === undefined && top === undefined ? undefined : [...(top ?? []), ...(base ?? [])];
const scalar = <T>(base: T | undefined, top: T | undefined): T | undefined => top ?? base;

// One entry per channel: adding a channel to `Channels` without deciding how
// it layers is a compile error here, not a default that silently drops.
const combine: { [K in keyof Required<Channels>]: Combine<K> } = {
  state: (base, top) =>
    base === undefined && top === undefined
      ? undefined
      : { shared: { ...(base?.shared ?? {}), ...(top?.shared ?? {}) }, transient: { ...(base?.transient ?? {}), ...(top?.transient ?? {}) } },
  registeredObjectAttributes: map,
  esv: map,
  esvInState: scalar,
  esvUndeclared: scalar,
  headers: map,
  params: map,
  cookies: map,
  cookieName: scalar,
  http: list,
  openidmActions: list,
  openidmFailures: list,
  bindingOverrides: map,
  identityAttributes: map,
  // A declared session in either layer asks for one, as in mergeChannels.
  session: map,
  sessionPrincipal: scalar,
};

/** `top` over `base`, per channel; an undeclared channel stays undeclared. */
export function layerChannels(base: Channels | undefined, top: Channels | undefined): Channels {
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(combine) as (keyof Channels)[]) {
    const value = (combine[key] as Combine<typeof key>)(base?.[key], top?.[key]);
    if (value !== undefined) out[key] = value;
  }
  return out as Channels;
}

/**
 * A `defineSuite` that starts every suite from `defaults`:
 *
 *     export const defineSuite = withSuiteDefaults({ always: { esv: { "esv.common.url": "https://x" } } });
 */
export function withSuiteDefaults(defaults: SuiteDefaults) {
  return <TSchema extends z.ZodType>(spec: SuiteSpec<TSchema>): Suite<TSchema> => {
    if (spec.defaults === false) return defineSuite(spec);
    const libraries = defaults.libraries === undefined && spec.libraries === undefined
      ? undefined
      : { ...(defaults.libraries ?? {}), ...(spec.libraries ?? {}) };
    return defineSuite({
      ...spec,
      always: layerChannels(defaults.always, spec.always),
      ...(libraries === undefined ? {} : { libraries }),
    });
  };
}
