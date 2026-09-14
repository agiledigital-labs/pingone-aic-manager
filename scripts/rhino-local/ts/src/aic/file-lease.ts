/**
 * Owns the reusable AM graph and tenant-backed check/cleanup replay behind the
 * harness's framework-free lane port. The Vitest composition root constructs
 * this tenant-aware side of the seam.
 */
import { randomUUID } from "node:crypto";
import type { Case, RecordedEffects } from "../case/types.ts";
import { judge } from "../case/verdict.ts";
import type { LeaseLaneHooks } from "../harness/lease.ts";
import { tenantIdmHandle } from "./idm.ts";
import { conformChain, type ChainConformanceReport, type LocalChainResult } from "./conform.ts";
import { emitLeasedJourney, type WrapperJourney } from "./emit-journey.ts";
import { emitLeasedSessionJourney } from "./emit-session.ts";
import { instrumentSubject } from "./emit-subject.ts";
import { createLeaseIdentity, type LeaseIdentity } from "./lease-identity.ts";
import {
  acquireLeaseLock,
  addJournalFixture,
  addJournalResources,
  leaseStatePaths,
  newLeaseJournal,
  readLeaseJournal,
  removeLeaseJournal,
  removeJournalFixture,
  writeLeaseJournal,
  type LeaseLock,
  type LeaseStatePaths,
} from "./lease-lock.ts";
import {
  acquireManagedFixtureLock,
  deleteManagedFixture,
  fixtureIdentity,
  managedResource,
  seedManagedFixtures,
  type ManagedFixture,
  type SeededManagedFixture,
} from "./managed.ts";
import { confirmResourceSnapshot } from "./resource-snapshot.ts";
import {
  deleteCreatedResources,
  driveJourney,
  fetchCookieName,
  invokeJourney,
  provisionJourney,
  validateAicRun,
  type AicPassObserver,
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
import { beginSingleTrace, beginTrace, clearAicTrace } from "./trace.ts";
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
  hooks: LeaseLaneHooks;
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
  #sessionWrapper: WrapperJourney | undefined;
  #sessionCreated: CreatedResource[] = [];
  #cookieName: string | undefined;
  #seededManaged: SeededManagedFixture[] = [];
  #releaseManagedLock: (() => Promise<void>) | undefined;

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
        aic: (args) =>
          this.#runEffects(
            args.cases,
            args.source,
            args.replies,
            args.managedFixtures ?? [],
            request.hooks
          ),
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
    const errors = await this.#cleanupManaged();
    if (errors.length > 0) {
      throw new AicLaneError(`AIC managed fixture cleanup failed: ${errors.join("; ")}`);
    }
  }

  async close(): Promise<void> {
    if (this.#state === "closed") {
      return;
    }
    await this.#queue;
    const errors: string[] = [];
    try {
      const session = this.#session;
      errors.push(...(await this.#cleanupManaged()));
      if (session !== undefined) {
        errors.push(
          ...(await deleteCreatedResources(
            this.#io,
            session,
            this.#realm,
            this.#sessionCreated
          ))
        );
        errors.push(
          ...(await deleteCreatedResources(
            this.#io,
            session,
            this.#realm,
            this.#created
          ))
        );
      }
      if (errors.length === 0) {
        this.#created = [];
        this.#sessionCreated = [];
        if (this.#paths !== undefined) {
          await removeLeaseJournal(this.#paths.journalPath);
        }
      }
    } finally {
      this.#state = "closed";
      await this.#releaseManagedLock?.();
      this.#releaseManagedLock = undefined;
      await this.#lock?.release();
    }
    if (errors.length > 0) {
      throw new AicLaneError(`AIC file lease cleanup failed: ${errors.join("; ")}`);
    }
  }

  async #runEffects(
    cases: readonly Case[],
    source: string,
    replies: readonly (readonly AicReply[])[],
    managedFixtures: readonly ManagedFixture[],
    hooks: LeaseLaneHooks
  ): Promise<readonly RecordedEffects[]> {
    const session = this.#session as TenantSession;
    // TODO(live): establish whether `aic whoami --token` refreshes a
    // long-lived file reliably and when refresh is required. No threshold is
    // assumed here; an expired bearer makes the authenticated request fail.
    if (managedFixtures.length > 0) {
      this.#releaseManagedLock = await acquireManagedFixtureLock(
        session,
        cases[cases.length - 1]?.name ?? this.identity.id
      );
      const identities = managedFixtures.map((fixture) => fixtureIdentity(fixture));
      for (const [index, identity] of identities.entries()) {
        const fixture = managedFixtures[index] as ManagedFixture;
        await addJournalFixture(this.#journalPath(), {
          type: fixture.type,
          id: identity.id,
        });
      }
    }
    let effects: readonly RecordedEffects[] | undefined;
    let failure: unknown;
    let fixturesReady = false;
    try {
      await seedManagedFixtures(
        this.#io,
        session,
        managedFixtures,
        this.#seededManaged
      );
      fixturesReady = true;
      let sessionCookie: { name: string; value: string } | undefined;
      const existingSession = cases[0]?.given.existingSession;
      if (existingSession !== undefined) {
        sessionCookie = await this.#mintSession(existingSession);
        clearAicTrace();
      }
      effects = await this.#armAndDrive(
        cases,
        source,
        replies,
        hooks,
        sessionCookie
      );
    } catch (error) {
      failure = error;
    }
    if (fixturesReady && hooks.cleanup !== undefined) {
      try {
        await hooks.cleanup(tenantIdmHandle(this.#io, session));
      } catch (error) {
        const cleanupFailure = laneHookError(
          cases[cases.length - 1]?.name ?? this.identity.id,
          "cleanup()",
          error
        );
        failure = combineHookFailure(failure, cleanupFailure);
      }
    }
    const cleanupErrors = await this.#cleanupManaged();
    for (const fixture of managedFixtures) {
      const identity = fixtureIdentity(fixture);
      if (
        !this.#seededManaged.some(
          (seeded) => seeded.type === fixture.type && seeded.id === identity.id
        )
      ) {
        try {
          await removeJournalFixture(this.#journalPath(), {
            type: fixture.type,
            id: identity.id,
          });
        } catch (error) {
          cleanupErrors.push(error instanceof Error ? error.message : String(error));
        }
      }
    }
    if (failure !== undefined) {
      if (cleanupErrors.length > 0) {
        throw new AggregateError(
          [failure, ...cleanupErrors.map((message) => new AicLaneError(message))],
          "AIC run and managed fixture cleanup both failed"
        );
      }
      throw failure;
    }
    if (cleanupErrors.length > 0) {
      throw new AicLaneError(
        `AIC managed fixture cleanup failed: ${cleanupErrors.join("; ")}`
      );
    }
    return effects as readonly RecordedEffects[];
  }

  async #armAndDrive(
    cases: readonly Case[],
    source: string,
    replies: readonly (readonly AicReply[])[],
    hooks: LeaseLaneHooks,
    sessionCookie?: { name: string; value: string }
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
    if (sessionCookie !== undefined) {
      wrapper.invoke.cookies[sessionCookie.name] = sessionCookie.value;
    }
    const observePass: AicPassObserver = async (index, effects) => {
      const final = index === cases.length - 1;
      if (!final && !judge(cases[index] as Case, effects).pass) {
        return;
      }
      const stepCheck = hooks.stepChecks[index];
      const checks = final
        ? hooks.finalChecks
        : stepCheck === undefined
          ? []
          : [stepCheck];
      for (const [checkIndex, check] of checks.entries()) {
        try {
          await check(tenantIdmHandle(this.#io, session), effects);
        } catch (error) {
          const label = final
            ? `final check() ${checkIndex + 1}`
            : `step ${index + 1} check()`;
          throw laneHookError(cases[index]?.name ?? this.identity.id, label, error);
        }
      }
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
      },
      observePass
    );
  }

  async #mintSession(
    existingSession: Record<string, string>
  ): Promise<{ name: string; value: string }> {
    const session = this.#session as TenantSession;
    const emitted = emitLeasedSessionJourney(existingSession, {
      identity: this.identity,
      realm: this.#realm,
    });
    if (this.#sessionWrapper === undefined) {
      await addJournalResources(this.#journalPath(), staticResources(emitted));
      await provisionJourney(this.#io, session, emitted, this.#sessionCreated);
      this.#sessionWrapper = emitted;
    } else {
      // TODO(live): prove an updated reusable minter script is what the next
      // invocation executes. The confirming GET catches storage drift but not
      // an unmeasured compiled-script cache.
      const script = emitted.scripts[0] as WrapperJourney["scripts"][number];
      const body = scriptBody(script, this.identity.marker);
      const path = `${realmJsonPath(this.#realm)}/scripts/${script.id}`;
      const update = await amRequest(this.#io, session, {
        method: "PUT",
        path,
        headers: amConfigHeaders(),
        body: JSON.stringify(body),
      });
      if (update.status !== 200) {
        throw new AicLaneError(
          `session-minter update returned HTTP ${update.status}, expected 200`
        );
      }
      const confirmation = await amRequest(this.#io, session, {
        method: "GET",
        path,
        headers: amConfigHeaders(),
      });
      if (confirmation.status !== 200) {
        throw new AicLaneError(
          `session-minter confirming read returned HTTP ${confirmation.status}, expected 200`
        );
      }
      confirmResourceSnapshot("script", body, confirmation.body);
      this.#sessionWrapper.scripts[0] = script;
    }
    const response = await invokeJourney(
      this.#io,
      session,
      this.#sessionWrapper,
      beginSingleTrace(session.tenantName)
    );
    const tokenId = (response.body as { tokenId?: unknown }).tokenId;
    if (typeof tokenId !== "string" || tokenId.length === 0) {
      throw new AicLaneError("session-minter returned no tokenId");
    }
    this.#cookieName ??= await fetchCookieName(this.#io, session);
    return { name: this.#cookieName, value: tokenId };
  }

  async #cleanupManaged(): Promise<string[]> {
    const errors: string[] = [];
    for (const fixture of this.#seededManaged.slice().reverse()) {
      try {
        await deleteManagedFixture(this.#io, this.#session as TenantSession, fixture);
        await removeJournalFixture(this.#journalPath(), fixture);
        this.#seededManaged = this.#seededManaged.filter(
          (item) => item.type !== fixture.type || item.id !== fixture.id
        );
      } catch (error) {
        errors.push(error instanceof Error ? error.message : String(error));
      }
    }
    if (this.#releaseManagedLock !== undefined && this.#seededManaged.length === 0) {
      try {
        await this.#releaseManagedLock();
        this.#releaseManagedLock = undefined;
      } catch (error) {
        errors.push(error instanceof Error ? error.message : String(error));
      }
    }
    return errors;
  }

  #validate(request: AicLeaseRunRequest): void {
    const validation = validateAicRun(
      request.cases,
      request.replies,
      request.managedFixtures
    );
    if (request.hooks.stepChecks.length !== request.replies.length) {
      throw new AicLaneError(
        `AIC file lease received ${request.hooks.stepChecks.length} step hook slots for ${request.replies.length} steps`
      );
    }
    if (request.source !== this.#options.source) {
      throw new AicLaneError("AIC file lease source differs from the suite source used at open");
    }
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
    }
    if (
      validation.unsupported.length > 0 &&
      this.#options.unsupported !== "skip"
    ) {
      throw new AicLaneError(
        `AIC lane unsupported: ${validation.unsupported.join("; ")}`
      );
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
    if (this.#created.length === 0 && this.#paths !== undefined) {
      await removeLeaseJournal(this.#paths.journalPath);
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
    for (const fixture of journal.managedFixtures) {
      const response = await amRequest(this.#io, this.#session as TenantSession, {
        method: "GET",
        path: managedResource(fixture.type, fixture.id),
      });
      if (response.status === 404) {
        continue;
      }
      if (response.status >= 200 && response.status < 300) {
        residue.push(`${fixture.type}/${fixture.id}`);
        continue;
      }
      throw new AicLaneError(
        `could not probe older managed fixture ${fixture.type}/${fixture.id} (HTTP ${response.status})`
      );
    }
    if (residue.length > 0) {
      throw new AicLaneError(
        `AIC file lease ${JSON.stringify(journal.aicId)} has owned residue: ${residue.join(", ")}`
      );
    }
    await removeLeaseJournal(paths.journalPath);
  }

  #journalPath(): string {
    if (this.#paths === undefined) {
      throw new AicLaneError("AIC file lease journal path is unavailable");
    }
    return this.#paths.journalPath;
  }
}

class AicLaneHookError extends AicLaneError {}

function laneHookError(name: string, hook: string, error: unknown): AicLaneHookError {
  return new AicLaneHookError(
    `local and AIC lanes disagreed for ${JSON.stringify(name)}: local ${hook} passed; AIC ${hook} threw ${safeErrorKind(error)}`
  );
}

function combineHookFailure(
  failure: unknown,
  cleanupFailure: AicLaneHookError
): unknown {
  if (failure === undefined) {
    return cleanupFailure;
  }
  const message =
    failure instanceof AicLaneHookError
      ? `${failure.message}; ${cleanupFailure.message}`
      : "AIC run and suite cleanup both failed";
  return new AggregateError([failure, cleanupFailure], message);
}

function safeErrorKind(error: unknown): string {
  if (!(error instanceof Error)) {
    return "a non-Error value";
  }
  return /^(?:Error|[A-Za-z][A-Za-z0-9]*Error)$/.test(error.name)
    ? error.name
    : "an Error";
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

function scriptBody(
  script: WrapperJourney["scripts"][number],
  description: string
): Record<string, unknown> {
  return {
    _id: script.id,
    name: script.name,
    description,
    script: Buffer.from(script.source, "utf8").toString("base64"),
    default: false,
    language: "JAVASCRIPT",
    context: "AUTHENTICATION_TREE_DECISION_NODE",
    evaluatorVersion: "2.0",
  };
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
