/**
 * Loads the journeys file: one JSON document, the shape of `JourneyConfigInput`
 * (`realm`, `trees`, `nodes`, `scripts`) plus an optional `idm` key holding an
 * IDM `ConfigInput` for nodes that read or write managed objects (default: no
 * managed objects, no sync mappings). The `aic journey pull` directory layout
 * is not read; export it to this one file first.
 */
import { readFileSync } from "node:fs";
import { JourneyConfig } from "../journey/config.ts";
import type { JourneyConfigInput } from "../journey/types.ts";
import type { ConfigInput } from "../idm/types.ts";

export type LoadedJourneys = { readonly config: JourneyConfig; readonly idm: ConfigInput };

export function parseJourneys(text: string): LoadedJourneys {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new Error(`journeys file: not JSON: ${error instanceof Error ? error.message : String(error)}`, { cause: error });
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("journeys file: must be a JSON object");
  const { idm, ...rest } = value as Record<string, unknown>;
  const config = new JourneyConfig(rest as unknown as JourneyConfigInput);
  return { config, idm: (idm as ConfigInput | undefined) ?? { sync: { mappings: [] }, managed: { objects: [] } } };
}

export function loadJourneys(path: string): LoadedJourneys {
  return parseJourneys(readFileSync(path, "utf8"));
}
