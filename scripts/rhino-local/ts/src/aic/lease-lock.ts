/** Local process lock and identifier-only crash journal for an AIC file lease. */
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { randomUUID } from "node:crypto";
import type { LeaseIdentity } from "./lease-identity.ts";
import { sha256 } from "./lease-identity.ts";
import type { CreatedResource } from "./run.ts";
import { AicLaneError } from "./tenant.ts";

export interface LeaseJournal {
  version: 1;
  tenantHash: string;
  aicId: string;
  realm: string;
  treeName: string;
  ownerToken: string;
  resources: CreatedResource[];
  managedFixtures: Array<{ type: string; id: string }>;
}

interface LockOwner {
  pid: number;
  ownerToken: string;
  aicId: string;
}

export interface LeaseStatePaths {
  root: string;
  lockDir: string;
  ownerPath: string;
  journalPath: string;
  tenantHash: string;
}

export interface LeaseLock {
  paths: LeaseStatePaths;
  release(): Promise<void>;
}

export function leaseStatePaths(
  baseUrl: string,
  identity: LeaseIdentity,
  root = join(tmpdir(), "rhino-local-aic-leases")
): LeaseStatePaths {
  const tenantHash = sha256(baseUrl).slice(0, 20);
  const key = `${tenantHash}-${identity.idHash}`;
  const lockDir = join(root, `${key}.lock`);
  return {
    root,
    lockDir,
    ownerPath: join(lockDir, "owner.json"),
    journalPath: join(root, `${key}.journal.json`),
    tenantHash,
  };
}

export async function acquireLeaseLock(
  paths: LeaseStatePaths,
  identity: LeaseIdentity,
  options: {
    timeoutMs?: number;
    processExists?: (pid: number) => boolean;
    pollMs?: number;
  } = {}
): Promise<LeaseLock> {
  await mkdir(paths.root, { recursive: true, mode: 0o700 });
  const deadline = Date.now() + (options.timeoutMs ?? 120_000);
  const isAlive = options.processExists ?? processExists;
  for (;;) {
    try {
      await mkdir(paths.lockDir, { mode: 0o700 });
      break;
    } catch (error) {
      if (!hasCode(error, "EEXIST")) {
        throw error;
      }
    }
    const owner = await readJson<LockOwner>(paths.ownerPath);
    if (owner !== undefined && !isAlive(owner.pid)) {
      await rm(paths.lockDir, { recursive: true, force: true });
      continue;
    }
    if (Date.now() >= deadline) {
      throw new AicLaneError(
        `timed out waiting for AIC file lease ${JSON.stringify(identity.id)}${owner === undefined ? "" : ` held by pid ${owner.pid}`}`
      );
    }
    await delay(options.pollMs ?? 100);
  }
  const owner: LockOwner = {
    pid: process.pid,
    ownerToken: identity.ownerToken,
    aicId: identity.id,
  };
  try {
    await writeFile(paths.ownerPath, JSON.stringify(owner), {
      encoding: "utf8",
      mode: 0o600,
      flag: "wx",
    });
  } catch (error) {
    await rm(paths.lockDir, { recursive: true, force: true });
    throw error;
  }
  let released = false;
  return {
    paths,
    async release() {
      if (released) {
        return;
      }
      const current = await readJson<LockOwner>(paths.ownerPath);
      if (current?.ownerToken === identity.ownerToken) {
        await rm(paths.lockDir, { recursive: true, force: true });
      }
      released = true;
    },
  };
}

export function newLeaseJournal(
  paths: LeaseStatePaths,
  identity: LeaseIdentity,
  realm: string,
  resources: readonly CreatedResource[]
): LeaseJournal {
  return {
    version: 1,
    tenantHash: paths.tenantHash,
    aicId: identity.id,
    realm,
    treeName: identity.treeName,
    ownerToken: identity.ownerToken,
    resources: resources.map((resource) => ({ ...resource })),
    managedFixtures: [],
  };
}

export async function readLeaseJournal(
  path: string
): Promise<LeaseJournal | undefined> {
  return readJson<LeaseJournal>(path);
}

export async function writeLeaseJournal(
  path: string,
  journal: LeaseJournal
): Promise<void> {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.${process.pid}.${randomUUID()}.tmp`;
  await writeFile(temporary, JSON.stringify(journal), {
    encoding: "utf8",
    mode: 0o600,
    flag: "wx",
  });
  await rename(temporary, path);
}

export async function removeLeaseJournal(path: string): Promise<void> {
  await rm(path, { force: true });
}

export async function addJournalFixture(
  path: string,
  fixture: { type: string; id: string }
): Promise<void> {
  const journal = await requireJournal(path);
  if (
    !journal.managedFixtures.some(
      (item) => item.type === fixture.type && item.id === fixture.id
    )
  ) {
    journal.managedFixtures.push(fixture);
    await writeLeaseJournal(path, journal);
  }
}

export async function removeJournalFixture(
  path: string,
  fixture: { type: string; id: string }
): Promise<void> {
  const journal = await requireJournal(path);
  journal.managedFixtures = journal.managedFixtures.filter(
    (item) => item.type !== fixture.type || item.id !== fixture.id
  );
  await writeLeaseJournal(path, journal);
}

async function requireJournal(path: string): Promise<LeaseJournal> {
  const journal = await readLeaseJournal(path);
  if (journal === undefined) {
    throw new AicLaneError("AIC file lease ownership journal is missing");
  }
  return journal;
}

async function readJson<T>(path: string): Promise<T | undefined> {
  try {
    return JSON.parse(await readFile(path, "utf8")) as T;
  } catch (error) {
    if (hasCode(error, "ENOENT")) {
      return undefined;
    }
    throw new AicLaneError(`could not read AIC file lease state at ${path}`, {
      cause: error,
    });
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
