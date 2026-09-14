import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createLeaseIdentity } from "../../src/aic/lease-identity.ts";
import {
  acquireLeaseLock,
  addJournalFixture,
  leaseStatePaths,
  newLeaseJournal,
  readLeaseJournal,
  writeLeaseJournal,
} from "../../src/aic/lease-lock.ts";

const temporary: string[] = [];

afterEach(async () => {
  for (const path of temporary.splice(0)) {
    await rm(path, { recursive: true, force: true });
  }
});

describe("AIC file lease local state", () => {
  it("reaps a dead-pid lock and acquires it", async () => {
    const root = await tempRoot();
    const identity = createLeaseIdentity({
      id: "dead-owner",
      source: "source",
      outcomes: ["done"],
      ownerToken: "new-owner",
    });
    const paths = leaseStatePaths("https://tenant.example.com", identity, root);
    await mkdir(paths.lockDir, { recursive: true });
    await writeFile(
      paths.ownerPath,
      JSON.stringify({ pid: 999_999, ownerToken: "dead", aicId: identity.id })
    );
    const lock = await acquireLeaseLock(paths, identity, {
      timeoutMs: 0,
      processExists: () => false,
    });
    expect(JSON.parse(await readFile(paths.ownerPath, "utf8"))).toMatchObject({
      ownerToken: "new-owner",
    });
    await lock.release();
  });

  it("fails clearly while a live owner holds the same lease", async () => {
    const root = await tempRoot();
    const identity = createLeaseIdentity({
      id: "live-owner",
      source: "source",
      outcomes: ["done"],
      ownerToken: "owner-a",
    });
    const paths = leaseStatePaths("https://tenant.example.com", identity, root);
    const first = await acquireLeaseLock(paths, identity);
    const contender = createLeaseIdentity({
      id: identity.id,
      source: "source",
      outcomes: ["done"],
      ownerToken: "owner-b",
    });
    await expect(
      acquireLeaseLock(paths, contender, {
        timeoutMs: 0,
        processExists: () => true,
      })
    ).rejects.toThrow(/timed out.*live-owner.*pid/);
    await first.release();
  });

  it("writes only identifiers and hashes to the atomic journal", async () => {
    const root = await tempRoot();
    const identity = createLeaseIdentity({
      id: "journal-safe-id",
      source: "source-must-not-land",
      outcomes: ["done"],
      ownerToken: "owner-token",
    });
    const baseUrl = "https://tenant.example.com";
    const paths = leaseStatePaths(baseUrl, identity, root);
    await writeLeaseJournal(
      paths.journalPath,
      newLeaseJournal(paths, identity, "alpha", [
        { kind: "script", id: identity.ids.subjectScript },
      ])
    );
    await addJournalFixture(paths.journalPath, {
      type: "managed/alpha_user",
      id: "fixture-id",
    });
    const text = await readFile(paths.journalPath, "utf8");
    expect(text).not.toContain(baseUrl);
    expect(text).not.toContain("tenant.example.com");
    expect(text).not.toContain("source-must-not-land");
    expect(text).not.toContain("seed-value");
    expect(text).not.toContain("bearer-value");
    expect(await readLeaseJournal(paths.journalPath)).toMatchObject({
      tenantHash: paths.tenantHash,
      managedFixtures: [{ type: "managed/alpha_user", id: "fixture-id" }],
    });
  });
});

async function tempRoot(): Promise<string> {
  const path = await mkdtemp(join(tmpdir(), "rhino-local-lease-test-"));
  temporary.push(path);
  return path;
}
