#!/usr/bin/env node
/**
 * Pull an environment's managed-object schema into a harness profile.
 *
 *   rhino-local-pull-profile [--tenant <name>] [--provider-module <file>]
 *
 * The tenant and bearer come from the configured tenant provider: the module
 * named by `--provider-module`, else the RHINO_LOCAL_TENANT_URL environment,
 * else the current `aic` context. The bearer is never written to disk.
 *
 * Only counts are printed. The object names are the tenant's business
 * vocabulary and belong in the profile, not in a terminal or CI log.
 */
import { parseArgs } from "node:util";
import { pullProfile } from "../src/profile/pull.ts";
import { loadProviderModule } from "../src/provider-module.ts";

try {
  const { values } = parseArgs({
    options: {
      tenant: { type: "string" },
      "provider-module": { type: "string" },
    },
  });
  if (values["provider-module"] !== undefined) {
    await loadProviderModule(values["provider-module"]);
  }
  const { profile, path } = await pullProfile(
    values.tenant !== undefined ? { tenant: values.tenant } : {}
  );
  const names = Object.keys(profile.objects);
  const fields = names.reduce(
    (total, name) => total + Object.keys(profile.objects[name]!.properties).length,
    0
  );
  console.log(`pulled   ${profile.pulledAt}`);
  console.log(`objects  ${names.length} (${fields} properties)`);
  console.log(`written  ${path}`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
