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
    const session = this.#session;
    const errors =
      session === undefined
        ? []
        : await deleteCreatedResources(this.#io, session, this.#realm, this.#created);
    if (errors.length === 0) {
      this.#created = [];
    }
    this.#state = "closed";
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
    if (this.#session === undefined || this.#created.length === 0) {
      return;
    }
    const errors = await deleteCreatedResources(
      this.#io,
      this.#session,
      this.#realm,
      this.#created
    );
    if (errors.length === 0) {
      this.#created = [];
    }
  }
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
