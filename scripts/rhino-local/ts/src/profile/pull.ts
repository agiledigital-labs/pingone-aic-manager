import {
  AicLaneError,
  amRequest,
  connectTenant,
  defaultAicIo,
  type AicIo,
} from "../aic/tenant.ts";
import { repoRoot } from "../paths.ts";
import { normaliseManagedConfig } from "./normalise.ts";
import { writeProfile } from "./store.ts";
import type { EnvProfile } from "./types.ts";

export const MANAGED_CONFIG_ENDPOINT = "/openidm/config/managed";

export interface PullOptions {
  tenant?: string;
  project?: string;
  io?: AicIo;
  now?: () => Date;
}

/**
 * Pull one environment's managed-object schema into a normalised profile.
 *
 * One call covers every managed object the tenant defines, which is why this
 * is worth more than per-object fixtures: the breadth comes free.
 */
export async function pullProfile(
  options: PullOptions = {}
): Promise<{ profile: EnvProfile; path: string }> {
  const project = options.project ?? repoRoot;
  const io = options.io ?? defaultAicIo(project);
  const session = await connectTenant(io, {
    ...(options.tenant !== undefined ? { tenant: options.tenant } : {}),
    project,
  });
  const response = await amRequest(io, session, {
    method: "GET",
    path: MANAGED_CONFIG_ENDPOINT,
  });
  if (response.status !== 200) {
    throw new AicLaneError(
      `GET ${MANAGED_CONFIG_ENDPOINT} returned ${response.status}: ${response.bodyText.slice(0, 300)}`,
      { status: response.status }
    );
  }
  const now = (options.now ?? (() => new Date()))();
  const profile = normaliseManagedConfig(response.body, {
    tenant: session.tenantName,
    pulledAt: now.toISOString(),
    endpoint: MANAGED_CONFIG_ENDPOINT,
  });
  const path = writeProfile(profile, project);
  return { profile, path };
}
