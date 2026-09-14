/**
 * Tenant-backed `IdmHandle` for replaying harness checks and cleanup. It keeps
 * the tenant's materialized record shape intact; the local mock's projection
 * is deliberately not reproduced on this lane.
 */
import type { JsonObject, JsonValue } from "../case/types.ts";
import { isPlainObject } from "../case/util.ts";
import type { IdmHandle } from "../harness/types.ts";
import { splitResource } from "../harness/idm.ts";
import { AicLaneError, amRequest, type AicIo, type TenantSession } from "./tenant.ts";

export function tenantIdmHandle(io: AicIo, session: TenantSession): IdmHandle {
  return {
    async read(resource) {
      const response = await amRequest(io, session, {
        method: "GET",
        path: recordPath(resource),
      });
      if (response.status === 404) {
        return null;
      }
      if (response.status !== 200 || !isPlainObject(response.body)) {
        throw statusError("read managed record", response.status, "200 or 404");
      }
      return response.body as JsonObject;
    },

    async query(type, filter) {
      const queryFilter = encodeQueryFilter(filter);
      const response = await amRequest(io, session, {
        method: "GET",
        path: `${collectionPath(type)}?_queryFilter=${encodeURIComponent(queryFilter)}`,
      });
      if (response.status !== 200 || !isPlainObject(response.body)) {
        throw statusError("query managed records", response.status, "200");
      }
      const result = response.body.result;
      const resultCount = response.body.resultCount;
      if (
        !Array.isArray(result) ||
        !result.every(isPlainObject) ||
        typeof resultCount !== "number"
      ) {
        throw new AicLaneError(
          "query managed records returned an invalid result/resultCount shape"
        );
      }
      return result as JsonObject[];
    },

    async delete(resource) {
      const response = await amRequest(io, session, {
        method: "DELETE",
        path: recordPath(resource),
      });
      if (response.status === 404) {
        return;
      }
      if (response.status !== 200 || !isPlainObject(response.body)) {
        throw statusError("delete managed record", response.status, "200 or 404");
      }
    },
  };
}

export function encodeQueryFilter(
  filter: Readonly<Record<string, JsonValue>>
): string {
  const entries = Object.entries(filter);
  if (entries.length === 0) {
    return "true";
  }
  return entries
    .map(([field, value]) => `${encodeField(field)} eq ${encodeFilterValue(field, value)}`)
    .join(" and ");
}

function recordPath(resource: string): string {
  const [type, id] = splitResource(resource);
  return `${collectionPath(type)}/${encodeURIComponent(id)}`;
}

function collectionPath(type: string): string {
  const match = /^managed\/([^/]+)$/.exec(type);
  if (match === null || match[1] === undefined) {
    throw new AicLaneError(
      `managed type ${JSON.stringify(type)} must have the form managed/<type>`
    );
  }
  return `/openidm/managed/${encodeURIComponent(match[1])}`;
}

function encodeField(field: string): string {
  if (!/^\/?[A-Za-z0-9_$.-]+(?:\/[A-Za-z0-9_$.-]+)*$/.test(field)) {
    throw new AicLaneError(
      `managed query field ${JSON.stringify(field)} cannot be encoded safely`
    );
  }
  return field;
}

function encodeFilterValue(field: string, value: JsonValue): string {
  if (typeof value === "string") {
    if (value.includes('"') || hasControlCharacter(value)) {
      throw unsupportedFilterValue(field, "strings containing quotes or control characters");
    }
    return `"${value}"`;
  }
  if (typeof value === "boolean") {
    return String(value);
  }
  if (typeof value === "number" && Number.isFinite(value)) {
    return String(value);
  }
  if (value === null) {
    throw unsupportedFilterValue(field, "null (CREST has no null literal)");
  }
  throw unsupportedFilterValue(field, "arrays and objects");
}

function hasControlCharacter(value: string): boolean {
  return [...value].some((character) => character.charCodeAt(0) < 0x20);
}

function unsupportedFilterValue(field: string, detail: string): AicLaneError {
  return new AicLaneError(
    `managed query field ${JSON.stringify(field)} uses an unencodable value: ${detail}`
  );
}

function statusError(operation: string, status: number, expected: string): AicLaneError {
  return new AicLaneError(
    `${operation} returned HTTP ${status}, expected ${expected}`,
    { status }
  );
}
