/**
 * Owns the reusable AM graph behind the harness's framework-free lane port.
 * The Vitest composition root constructs this tenant-aware side of the seam.
 */
import { randomUUID } from "node:crypto";
import type { Case, RecordedEffects } from "../case/types.ts";
import { conformChain, type ChainConformanceReport, type LocalChainResult } from "./conform.ts";
import { emitLeasedJourney, type WrapperJourney } from "./emit-journey.ts";
import { instrumentSubject } from "./emit-subject.ts";
import { createLeaseIdentity, type LeaseIdentity } from "./lease-identity.ts";
import {
  acquireLeaseLock,
  leaseStatePaths,
  newLeaseJournal,
  readLeaseJournal,
  removeLeaseJournal,
  writeLeaseJournal,
  type LeaseLock,
  type LeaseStatePaths,
} from "./lease-lock.ts";
import { managedSeedMatches } from "./managed.ts";
import { confirmResourceSnapshot } from "./resource-snapshot.ts";
import {
  deleteCreatedResources,
  driveJourney,
  provisionJourney,
  type AicReply,
  type CreatedResource,
} from "./run.ts";
import {
  AicLaneError,
  amConfigHeaders,
  amRequest,
  connectTenant,
  defaultAicIo,
  realmJsonPath,
  type AicIo,
  type TenantSession,
} from "./tenant.ts";
import { beginTrace } from "./trace.ts";
import { aicUnsupportedReason } from "./unsupported.ts";
import { repoRoot } from "../paths.ts";

export interface AicFileLeaseOptions {
  id: string;
  suiteName: string;
  source: string;
  outcomes: readonly string[];
  realm?: string;
  tenant?: string;
  project?: string;
  unsupported?: "fail" | "skip";
  io?: AicIo;
  /** Test seam; production state defaults to the machine temp directory. */
  stateDir?: string;
  /** Test seam for proving a live lock fails closed without a long wait. */
  lockTimeoutMs?: number;
}

export interface AicLeaseRunRequest extends LocalChainResult {
  source: string;
}

export class AicFileLease {
  readonly #options: AicFileLeaseOptions;
  readonly #io: AicIo;
  readonly #realm: string;
  readonly #project: string;
  #identity: LeaseIdentity | undefined;
  #wrapper: WrapperJourney | undefined;
  #session: TenantSession | undefined;
  #created: CreatedResource[] = [];
  #state: "new" | "opening" | "open" | "closed" = "new";
  #queue: Promise<void> = Promise.resolve();
  #lock: LeaseLock | undefined;
  #paths: LeaseStatePaths | undefined;

  constructor(options: AicFileLeaseOptions) {
    this.#options = options;
    this.#realm = options.realm ?? "alpha";
    this.#project = options.project ?? repoRoot;
    this.#io = options.io ?? defaultAicIo(this.#project);
  }

  get identity(): LeaseIdentity {
    if (this.#identity === undefined) {
      throw new AicLaneError("AIC file lease has not been opened");
    }
    return this.#identity;
  }

  async open(): Promise<void> {
    if (this.#state !== "new") {
      throw new AicLaneError(`AIC file lease cannot open from state ${this.#state}`);
    }
    this.#state = "opening";
    this.#identity = createLeaseIdentity({
      id: this.#options.id,
      source: this.#options.source,
      outcomes: this.#options.outcomes,
    });
    this.#wrapper = emitLeasedJourney({
      identity: this.#identity,
      realm: this.#realm,
      suiteName: this.#options.suiteName,
    });
    try {
      this.#session = await connectTenant(this.#io, {
        ...(this.#options.tenant === undefined ? {} : { tenant: this.#options.tenant }),
        project: this.#project,
      });
      this.#paths = leaseStatePaths(
        this.#session.baseUrl,
        this.#identity,
        this.#options.stateDir
      );
      this.#lock = await acquireLeaseLock(this.#paths, this.#identity, {
        ...(this.#options.lockTimeoutMs === undefined
          ? {}
          : { timeoutMs: this.#options.lockTimeoutMs }),
      });
      await this.#handleOlderJournal();
      await writeLeaseJournal(
        this.#paths.journalPath,
        newLeaseJournal(
          this.#paths,
          this.#identity,
          this.#realm,
          staticResources(this.#wrapper)
        )
      );
      await provisionJourney(this.#io, this.#session, this.#wrapper, this.#created);
      this.#state = "open";
    } catch (error) {
      await this.#cleanupAfterFailedOpen();
      this.#state = "closed";
      throw error;
    }
  }

  run(request: AicLeaseRunRequest): Promise<ChainConformanceReport> {
    const task = async (): Promise<ChainConformanceReport> => {
      this.#requireOpen();
      this.#validate(request);
      const report = await conformChain({
        ...request,
        aic: (args) => this.#runEffects(args.cases, args.source, args.replies),
      });
      assertConformance(report);
      return report;
    };
    const result = this.#queue.then(task, task);
    this.#queue = result.then(
      () => undefined,
      () => undefined
    );
    return result;
  }

  async endTest(): Promise<void> {
    await this.#queue;
  }

  async close(): Promise<void> {
    if (this.#state === "closed") {
      return;
    }
    await this.#queue;
    let errors: string[] = [];
    try {
      const session = this.#session;
      errors =
        session === undefined
          ? []
          : await deleteCreatedResources(this.#io, session, this.#realm, this.#created);
      if (errors.length === 0) {
        this.#created = [];
        if (this.#paths !== undefined) {
          await removeLeaseJournal(this.#paths.journalPath);
        }
      }
    } finally {
      this.#state = "closed";
      await this.#lock?.release();
    }
    if (errors.length > 0) {
      throw new AicLaneError(`AIC file lease cleanup failed: ${errors.join("; ")}`);
    }
  }

  async #runEffects(
    cases: readonly Case[],
    source: string,
    replies: readonly (readonly AicReply[])[]
  ): Promise<readonly RecordedEffects[]> {
    const session = this.#session as TenantSession;
    const wrapper = this.#wrapper as WrapperJourney;
    const nonce = randomUUID();
    const instrumented = instrumentSubject(source, this.identity.idHash, cases[0]?.given, {
      snapshotKey: this.identity.snapshotKey,
      invocationNonce: nonce,
      header: this.identity.marker,
    });
    const subjectDigest = instrumented.subjectDigest;
    if (subjectDigest === undefined) {
      throw new AicLaneError("leased subject instrumentation produced no digest");
    }
    // TODO(live): measure the maximum accepted script size for create and update.
    const body = {
      _id: this.identity.ids.subjectScript,
      name: `${this.identity.treeName}-subject`,
      description: this.identity.marker,
      script: Buffer.from(instrumented.source, "utf8").toString("base64"),
      default: false,
      language: "JAVASCRIPT",
      context: "AUTHENTICATION_TREE_DECISION_NODE",
      evaluatorVersion: "2.0",
    };
    const path = `${realmJsonPath(this.#realm)}/scripts/${this.identity.ids.subjectScript}`;
    const update = await amRequest(this.#io, session, {
      method: "PUT",
      path,
      headers: amConfigHeaders(),
      body: JSON.stringify(body),
    });
    if (update.status !== 200) {
      throw new AicLaneError(
        `subject update returned HTTP ${update.status}, expected 200; the owned slot may have disappeared`
      );
    }
    const confirmation = await amRequest(this.#io, session, {
      method: "GET",
      path,
      headers: amConfigHeaders(),
    });
    if (confirmation.status !== 200) {
      throw new AicLaneError(
        `subject confirming read returned HTTP ${confirmation.status}, expected 200`
      );
    }
    confirmResourceSnapshot("script", body, confirmation.body);
    const first = cases[0] as Case;
    wrapper.invoke = {
      headers: copyArrayMap(first.given.requestHeaders),
      parameters: copyArrayMap(first.given.requestParameters),
      cookies: { ...(first.given.requestCookies ?? {}) },
    };
    return driveJourney(
      this.#io,
      session,
      wrapper,
      cases,
      replies,
      beginTrace(session.tenantName),
      {
        leaseDigest: this.identity.structuralDigest,
        invocationNonce: nonce,
        subjectDigest,
      }
    );
  }

  #validate(request: AicLeaseRunRequest): void {
    if (request.cases.length === 0) {
      throw new AicLaneError("AIC file lease needs at least one case");
    }
    if (request.source !== this.#options.source) {
      throw new AicLaneError("AIC file lease source differs from the suite source used at open");
    }
    if (request.replies.length !== request.cases.length - 1) {
      throw new AicLaneError(
        `AIC file lease: ${request.cases.length} passes need ${request.cases.length - 1} reply sets, got ${request.replies.length}`
      );
    }
    if ((request.managedFixtures?.length ?? 0) > 0) {
      throw new AicLaneError("managed fixtures are not available on this file-lease slice");
    }
    if (request.cases.some((kase) => kase.given.existingSession !== undefined)) {
      throw new AicLaneError("existingSession is not available on this file-lease slice");
    }
    const harnessOwnsManaged =
      request.managedFixtures !== undefined &&
      managedSeedMatches(request.cases[0]?.given.managed, request.managedFixtures);
    for (const kase of request.cases) {
      if (kase.script !== this.#options.source) {
        throw new AicLaneError(`${kase.name}: case source differs from the leased suite source`);
      }
      const realm = kase.given.realm ?? "alpha";
      if (realm !== this.#realm) {
        throw new AicLaneError(
          `${kase.name}: realm ${JSON.stringify(realm)} differs from leased realm ${JSON.stringify(this.#realm)}`
        );
      }
      if (
        kase.expect.outcome !== null &&
        !this.identity.outcomes.includes(kase.expect.outcome)
      ) {
        throw new AicLaneError(
          `${kase.name}: outcome ${JSON.stringify(kase.expect.outcome)} is outside the leased vocabulary`
        );
      }
      const unsupported = aicUnsupportedReason(kase, { harnessOwnsManaged });
      if (unsupported !== undefined && this.#options.unsupported !== "skip") {
        throw new AicLaneError(`${kase.name}: AIC lane unsupported: ${unsupported}`);
      }
    }
  }

  #requireOpen(): void {
    if (this.#state !== "open") {
      throw new AicLaneError(`AIC file lease is not open (state ${this.#state})`);
    }
  }

  async #cleanupAfterFailedOpen(): Promise<void> {
    if (this.#session !== undefined && this.#created.length > 0) {
      const errors = await deleteCreatedResources(
        this.#io,
        this.#session,
        this.#realm,
        this.#created
      );
      if (errors.length === 0) {
        this.#created = [];
        if (this.#paths !== undefined) {
          await removeLeaseJournal(this.#paths.journalPath);
        }
      }
    }
    await this.#lock?.release();
  }

  async #handleOlderJournal(): Promise<void> {
    const paths = this.#paths as LeaseStatePaths;
    const journal = await readLeaseJournal(paths.journalPath);
    if (journal === undefined) {
      return;
    }
    const residue: string[] = [];
    for (const resource of journal.resources) {
      const path = resourcePath(journal.realm, resource);
      const response = await amRequest(this.#io, this.#session as TenantSession, {
        method: "GET",
        path,
        headers: amConfigHeaders(),
      });
      if (response.status === 404) {
        continue;
      }
      if (response.status >= 200 && response.status < 300) {
        residue.push(resourceLabel(resource));
        continue;
      }
      throw new AicLaneError(
        `could not probe older AIC lease journal resource ${resourceLabel(resource)} (HTTP ${response.status})`
      );
    }
    if (journal.managedFixtures.length > 0) {
      residue.push(
        ...journal.managedFixtures.map((fixture) => `${fixture.type}/${fixture.id}`)
      );
    }
    if (residue.length > 0) {
      throw new AicLaneError(
        `AIC file lease ${JSON.stringify(journal.aicId)} has owned residue: ${residue.join(", ")}`
      );
    }
    await removeLeaseJournal(paths.journalPath);
  }
}

function staticResources(wrapper: WrapperJourney): CreatedResource[] {
  return [
    ...wrapper.scripts.map((script) => ({ kind: "script" as const, id: script.id })),
    ...wrapper.nodes.map((node) => ({ kind: "node" as const, id: node.id })),
    { kind: "tree" as const, name: wrapper.treeName },
  ];
}

function resourcePath(realm: string, resource: CreatedResource): string {
  const base = realmJsonPath(realm);
  if (resource.kind === "script") {
    return `${base}/scripts/${resource.id}`;
  }
  if (resource.kind === "node") {
    return `${base}/realm-config/authentication/authenticationtrees/nodes/ScriptedDecisionNode/${resource.id}`;
  }
  return `${base}/realm-config/authentication/authenticationtrees/trees/${encodeURIComponent(resource.name)}`;
}

function resourceLabel(resource: CreatedResource): string {
  return resource.kind === "tree"
    ? `tree ${resource.name}`
    : `${resource.kind} ${resource.id}`;
}

function assertConformance(report: ChainConformanceReport): void {
  const error = report.passes.find((pass) => pass.aic.error !== undefined)?.aic.error;
  if (error !== undefined) {
    throw new AicLaneError(error);
  }
  const failed = report.passes.find((pass) => pass.aic.verdict?.pass === false);
  if (failed !== undefined) {
    throw new AicLaneError(`AIC verdict failed for ${failed.name}: ${failed.aic.verdict?.summary}`);
  }
  if (report.disagreements.length > 0) {
    throw new AicLaneError(
      `local and AIC lanes disagreed: ${report.disagreements.map((item) => item.message).join("; ")}`
    );
  }
}

function copyArrayMap(
  value: Record<string, string[]> | undefined
): Record<string, string[]> {
  return Object.fromEntries(
    Object.entries(value ?? {}).map(([key, items]) => [key, items.slice()])
  );
}
