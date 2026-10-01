/**
 * Tenant-backed `IdmHandle` for replaying harness checks and cleanup. It keeps
 * the tenant's materialized record shape intact; the local mock's projection
 * is deliberately not reproduced on this lane.
 */
import type { JsonValue } from "../case/types.ts";
import type { IdmHandle } from "../harness/types.ts";
import { type AicIo, type TenantSession } from "./tenant.ts";
export declare function tenantIdmHandle(io: AicIo, session: TenantSession): IdmHandle;
export declare function encodeQueryFilter(filter: Readonly<Record<string, JsonValue>>): string;
