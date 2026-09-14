import { createHash } from "node:crypto";
import { mkdir, readFile, rmdir, stat, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { deepEqual } from "../case/equal.ts";
import type { Given, JsonObject } from "../case/types.ts";
import { isPlainObject } from "../case/util.ts";
import { AicLaneError, amRequest, type AicIo, type TenantSession } from "./tenant.ts";

export interface ManagedFixture {
  type: string;
  record: JsonObject;
}

export interface SeededManagedFixture {
  type: string;
  id: string;
}

interface LockOwner {
  pid: number;
  label: string;
}

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * Fields the maintainer measured as accepted on create but omitted by GET.
 * Additions need their own live evidence; skipping every absent field would
 * let a silently dropped readable value pass the seed check.
 */
const READBACK_EXEMPT_FIELDS: Readonly<Record<string, readonly string[]>> = {
  "managed/alpha_user": ["password"],
};

/** Whether the first pass's managed seed came exactly from this fixture ledger. */
export function managedSeedMatches(
  managed: Given["managed"],
  fixtures: readonly ManagedFixture[]
): boolean {
  const seeded: NonNullable<Given["managed"]> = {};
  for (const fixture of fixtures) {
    const rows = seeded[fixture.type] ?? [];
    rows.push(fixture.record);
    seeded[fixture.type] = rows;
  }
  return deepEqual(managed ?? {}, seeded);
}

/** Create with CREST's create-only request, then prove every declared field landed. */
export async function seedManagedFixtures(
  io: AicIo,
  session: TenantSession,
  fixtures: readonly ManagedFixture[],
  seeded: SeededManagedFixture[]
): Promise<void> {
  for (const fixture of fixtures) {
    const identity = fixtureIdentity(fixture);
    const response = await amRequest(io, session, {
      method: "POST",
      path: `${identity.collection}?_action=create`,
      body: JSON.stringify(fixture.record),
    });
    if (response.status === 412) {
      throw new AicLaneError(
        `managed fixture collision at ${identity.resource}: _id ${JSON.stringify(identity.id)} already exists; it may be a leaked fixture or tenant-owned record`
      );
    }
    if (response.status !== 201) {
      throw new AicLaneError(
        `create managed fixture ${identity.resource} returned HTTP ${response.status}: ${response.bodyText}`
      );
    }
    seeded.push({ type: fixture.type, id: identity.id });
    await checkSeededManaged(io, session, fixture, identity.resource);
  }
}

/** Delete one fixture the harness successfully created. A prior script delete is clean. */
export async function deleteManagedFixture(
  io: AicIo,
  session: TenantSession,
  fixture: SeededManagedFixture
): Promise<void> {
  const resource = managedResource(fixture.type, fixture.id);
  const response = await amRequest(io, session, {
    method: "DELETE",
    path: resource,
  });
  if (response.status === 404 || (response.status >= 200 && response.status < 300)) {
    return;
  }
  throw new AicLaneError(
    `delete managed fixture ${resource} returned HTTP ${response.status}: ${response.bodyText}`
  );
}

/**
 * Serialize managed-fixture runs across worker processes for one tenant.
 * A collision after this lock is acquired is therefore pre-existing state,
 * not two files in this harness racing each other.
 */
export async function acquireManagedFixtureLock(
  session: TenantSession,
  label: string
): Promise<() => Promise<void>> {
  const tenantKey = createHash("sha256")
    .update(session.baseUrl)
    .digest("hex")
    .slice(0, 20);
  const lockPath = join(tmpdir(), `rhino-local-managed-${tenantKey}.lock`);
  const ownerPath = join(lockPath, "owner.json");
  const deadline = Date.now() + 120_000;
  for (;;) {
    let acquired = false;
    try {
      await mkdir(lockPath, { mode: 0o700 });
      acquired = true;
    } catch (error) {
      if (!hasCode(error, "EEXIST")) {
        throw error;
      }
    }
    if (acquired) {
      try {
        await writeFile(
          ownerPath,
          JSON.stringify({ pid: process.pid, label } satisfies LockOwner),
          { mode: 0o600 }
        );
      } catch (error) {
        await removeLock(lockPath, ownerPath);
        throw error;
      }
      return async () => removeLock(lockPath, ownerPath);
    }

    const owner = await readOwner(ownerPath);
    if (owner !== undefined && !processExists(owner.pid)) {
      await removeLock(lockPath, ownerPath);
      continue;
    }
    if (owner === undefined && (await lockIsStale(lockPath))) {
      await removeLock(lockPath, ownerPath);
      continue;
    }
    if (Date.now() >= deadline) {
      throw new AicLaneError(
        `timed out waiting for managed-fixture tenant lock${owner === undefined ? "" : ` held by ${JSON.stringify(owner.label)}`}`
      );
    }
    await delay(100);
  }
}

async function checkSeededManaged(
  io: AicIo,
  session: TenantSession,
  fixture: ManagedFixture,
  resource: string
): Promise<void> {
  const response = await amRequest(io, session, { method: "GET", path: resource });
  if (response.status < 200 || response.status >= 300 || !isPlainObject(response.body)) {
    throw new AicLaneError(
      `managed fixture ${resource} was not readable after create (HTTP ${response.status})`
    );
  }
  for (const [key, expected] of Object.entries(fixture.record)) {
    const present = Object.prototype.hasOwnProperty.call(response.body, key);
    if (
      !present &&
      READBACK_EXEMPT_FIELDS[fixture.type]?.includes(key) === true
    ) {
      continue;
    }
    if (!present || !deepEqual(response.body[key], expected)) {
      throw new AicLaneError(
        `managed fixture ${resource} did not contain the declared field ${JSON.stringify(key)} after create`
      );
    }
  }
}

export function fixtureIdentity(fixture: ManagedFixture): {
  collection: string;
  id: string;
  resource: string;
} {
  const match = /^managed\/([^/]+)$/.exec(fixture.type);
  if (match === null || match[1] === undefined) {
    throw new AicLaneError(
      `managed fixture type ${JSON.stringify(fixture.type)} must be managed/<type>`
    );
  }
  const id = fixture.record._id;
  if (typeof id !== "string" || id.length === 0) {
    throw new AicLaneError(
      `managed fixture ${fixture.type} needs a non-empty string _id`
    );
  }
  if (fixture.type === "managed/alpha_user" && !UUID_PATTERN.test(id)) {
    throw new AicLaneError(
      `managed/alpha_user fixture _id ${JSON.stringify(id)} must be a 36-character UUID because user _id is the fr-idm-uuid RDN; managed/alpha_role accepts readable ids instead`
    );
  }
  const collection = `/openidm/managed/${encodeURIComponent(match[1])}`;
  return { collection, id, resource: `${collection}/${encodeURIComponent(id)}` };
}

export function managedResource(type: string, id: string): string {
  const match = /^managed\/([^/]+)$/.exec(type);
  if (match === null || match[1] === undefined) {
    throw new AicLaneError(`managed fixture type ${JSON.stringify(type)} must be managed/<type>`);
  }
  return `/openidm/managed/${encodeURIComponent(match[1])}/${encodeURIComponent(id)}`;
}

async function readOwner(path: string): Promise<LockOwner | undefined> {
  try {
    const value = JSON.parse(await readFile(path, "utf8")) as Partial<LockOwner>;
    return typeof value.pid === "number" && typeof value.label === "string"
      ? { pid: value.pid, label: value.label }
      : undefined;
  } catch {
    return undefined;
  }
}

function processExists(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return hasCode(error, "EPERM");
  }
}

async function lockIsStale(path: string): Promise<boolean> {
  try {
    return Date.now() - (await stat(path)).mtimeMs > 5_000;
  } catch {
    return false;
  }
}

async function removeLock(lockPath: string, ownerPath: string): Promise<void> {
  try {
    await unlink(ownerPath);
  } catch (error) {
    if (!hasCode(error, "ENOENT")) {
      throw error;
    }
  }
  try {
    await rmdir(lockPath);
  } catch (error) {
    if (!hasCode(error, "ENOENT")) {
      throw error;
    }
  }
}

function hasCode(error: unknown, code: string): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === code
  );
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
