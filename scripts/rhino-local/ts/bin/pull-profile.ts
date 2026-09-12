/**
 * Pull an environment's managed-object schema into a harness profile.
 *
 *   npm run pull-profile                 # the current aic context
 *   npm run pull-profile -- --tenant foo
 *
 * Requires an unlocked agent (`aic login`) — the bearer is borrowed from it and
 * never written to disk.
 */
import { pullProfile } from "../src/profile/pull.ts";

const args = process.argv.slice(2);
const tenantFlag = args.indexOf("--tenant");
const tenant = tenantFlag >= 0 ? args[tenantFlag + 1] : undefined;

if (tenantFlag >= 0 && (tenant === undefined || tenant.startsWith("--"))) {
  console.error("pull-profile: --tenant needs a value");
  process.exit(2);
}

try {
  const { profile, path } = await pullProfile(
    tenant !== undefined ? { tenant } : {}
  );
  const names = Object.keys(profile.objects).sort();
  const fields = names.reduce(
    (total, name) => total + Object.keys(profile.objects[name]!.properties).length,
    0
  );
  console.log(`tenant   ${profile.tenant}`);
  console.log(`pulled   ${profile.pulledAt}`);
  console.log(`objects  ${names.length} (${fields} properties)`);
  console.log(`         ${names.join(", ")}`);
  console.log(`written  ${path}`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
