import type { TenantProvider } from "./provider.ts";
/**
 * Owns the reusable AM graph and tenant-backed check/cleanup replay behind the
 * harness's framework-free lane port. The Vitest composition root constructs
 * this tenant-aware side of the seam.
 */
import { randomUUID } from "node:crypto";
import { deepEqual } from "../case/equal.ts";
import type { Case, RecordedEffects } from "../case/types.ts";
import { judge } from "../case/verdict.ts";
import type { LeaseLaneHooks } from "../harness/lease.ts";
import { tenantIdmHandle } from "./idm.ts";
import { conformChain, type ChainConformanceReport, type LocalChainResult } from "./conform.ts";
import { emitLeasedJourney, type WrapperJourney } from "./emit-journey.ts";
import { emitLeasedSessionJourney } from "./emit-session.ts";
import { instrumentSubject } from "./emit-subject.ts";
import { createLeaseIdentity, parseLeaseMarker, sha256, uuidV5, type LeaseIdentity } from "./lease-identity.ts";
import {
  acquireLeaseLock,
  addJournalFixture,
  addJournalOwnedLibrary,
  addJournalResources,
  leaseStatePaths,
  markJournalLibraryNotOwned,
  newLeaseJournal,
  readLeaseJournal,
  removeLeaseJournal,
  removeJournalFixture,
  writeLeaseJournal,
  type LeaseLock,
  type LeaseStatePaths,
  type OwnedLibrary,
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
import { projectRoot } from "../project.ts";

export interface AicFileLeaseOptions {
  id: string;
  suiteName: string;
  source: string;
  outcomes: readonly string[];
  libraries?: Readonly<Record<string, string>>;
  realm?: string;
  tenant?: string;
  project?: string;
  /** Where the tenant and its bearer come from; see `resolveTenantProvider`. */
  provider?: TenantProvider;
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
  #ownedLibraries = new Map<string, OwnedLibrary>();
  #journalWrittenForOpen = false;
  #writeAttempted = false;
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
    this.#project = options.project ?? projectRoot();
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
        ...(this.#options.provider === undefined ? {} : { provider: this.#options.provider }),
        useConfigured: this.#options.io === undefined,
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
      const libraries = await this.#planLibraries();
      await writeLeaseJournal(
        this.#paths.journalPath,
        newLeaseJournal(
          this.#paths,
          this.#identity,
          this.#realm,
          staticResources(this.#wrapper)
        )
      );
      this.#journalWrittenForOpen = true;
      await this.#provisionLibraries(libraries);
      await provisionJourney(this.#io, this.#session, this.#wrapper, this.#created,
        () => { this.#writeAttempted = true; });
      this.#state = "open";
    } catch (error) {
      const cleanupErrors = await this.#cleanupAfterFailedOpen();
      this.#state = "closed";
      if (cleanupErrors.length > 0) {
        throw new AggregateError(
          [error, ...cleanupErrors.map((message) => new AicLaneError(message))],
          `AIC file lease open failed and cleanup left resources: ${cleanupErrors.join("; ")}`,
          { cause: error }
        );
      }
      throw error;
    }
  }

  run(request: AicLeaseRunRequest): Promise<ChainConformanceReport> {
    const task = async (): Promise<ChainConformanceReport> => {
      this.#requireOpen();
      this.#validate(request);
      const report = await conformChain({
        ...request,
        harnessOwnsScriptName: true,
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
          ...(await this.#deleteCreatedResourcesSafely())
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

  async #planLibraries(): Promise<Array<{ name: string; source: string; id: string }>> {
    const planned: Array<{ name: string; source: string; id: string }> = [];
    for (const [name, source] of Object.entries(this.#options.libraries ?? {})) {
      if (name.trim() === "") {
        throw new AicLaneError("AIC library name must not be empty");
      }
      if (typeof source !== "string") {
        throw new AicLaneError(`library ${JSON.stringify(name)} source must be a string`);
      }
      const filter = encodeURIComponent(`name eq ${JSON.stringify(name)}`);
      const path = `${realmJsonPath(this.#realm)}/scripts?_queryFilter=${filter}`;
      const response = await amRequest(this.#io, this.#session as TenantSession, {
        method: "GET", path, headers: amConfigHeaders(),
      });
      if (response.status !== 200) {
        throw new AicLaneError(`library ${JSON.stringify(name)} lookup returned HTTP ${response.status}`);
      }
      const result = (response.body as { result?: unknown }).result;
      if (!Array.isArray(result)) {
        throw new AicLaneError(`library ${JSON.stringify(name)} lookup returned no result array`);
      }
      const matches = result.filter((item): item is Record<string, unknown> =>
        typeof item === "object" && item !== null && (item as { name?: unknown }).name === name
      );
      if (matches.length > 1) {
        throw new AicLaneError(`library ${JSON.stringify(name)} lookup returned duplicate names`);
      }
      const existing = matches[0];
      if (existing !== undefined) {
        if (typeof existing.description === "string" &&
            existing.description.startsWith("rhino-local:")) {
          const owner = parseLeaseMarker(existing.description);
          const ownerName = owner?.id === undefined
            ? `aic.id hash ${JSON.stringify(owner?.idHash ?? "unknown")}`
            : `aic.id ${JSON.stringify(owner.id)}`;
          throw new AicLaneError(`library ${JSON.stringify(name)} belongs to an AIC file lease with ${ownerName}; give the libraries distinct names or use one aic.id`);
        }
        if (existing.context !== "LIBRARY" || typeof existing.script !== "string" ||
            Buffer.from(existing.script, "base64").toString("utf8") !== source) {
          throw new AicLaneError(`library ${JSON.stringify(name)} already exists with different context or source; refusing to overwrite it`);
        }
        continue;
      }
      planned.push({ name, source, id: uuidV5(`${this.identity.id}:library:${name}`) });
    }
    // AM's create-time require-target validation is unmeasured. Preserve the
    // caller's declaration order; close uses AM's measured delete refusal to
    // resolve dependencies without parsing source text.
    for (const library of planned) {
      const response = await amRequest(this.#io, this.#session as TenantSession, {
        method: "GET", path: resourcePath(this.#realm, { kind: "script", id: library.id }),
        headers: amConfigHeaders(),
      });
      if (response.status !== 404) {
        throw new AicLaneError(`library ${JSON.stringify(library.name)} id already exists or could not be checked (HTTP ${response.status}); refusing to overwrite it`);
      }
    }
    return planned;
  }

  async #provisionLibraries(libraries: readonly { name: string; source: string; id: string }[]): Promise<void> {
    for (const library of libraries) {
      const resource: CreatedResource = { kind: "script", id: library.id };
      const ownership: OwnedLibrary = {
        id: library.id, name: library.name,
        sourceHash: sha256(library.source), marker: this.identity.marker,
        status: "owned",
      };
      await addJournalOwnedLibrary(this.#journalPath(), ownership);
      const path = resourcePath(this.#realm, resource);
      const body = {
        _id: library.id, name: library.name, description: this.identity.marker,
        script: Buffer.from(library.source, "utf8").toString("base64"),
        default: false, language: "JAVASCRIPT", context: "LIBRARY",
        evaluatorVersion: "2.0",
      };
      // The journal entry exists before this write. Keep it if the response is
      // lost, since #created is filled only after a confirmed create status.
      this.#ownedLibraries.set(library.id, ownership);
      this.#writeAttempted = true;
      const response = await amRequest(this.#io, this.#session as TenantSession, {
        method: "PUT", path, headers: amConfigHeaders(), body: JSON.stringify(body),
      });
      if (response.status === 200) {
        ownership.status = "not-owned";
        await markJournalLibraryNotOwned(this.#journalPath(), library.id);
        throw new AicLaneError(collisionResidue(ownership));
      }
      if (response.status !== 201) {
        throw new AicLaneError(`create library ${JSON.stringify(library.name)} returned HTTP ${response.status}, expected 201`);
      }
      this.#created.push(resource);
      const confirmation = await amRequest(this.#io, this.#session as TenantSession, {
        method: "GET", path, headers: amConfigHeaders(),
      });
      if (confirmation.status !== 200) {
        throw new AicLaneError(`confirm library ${JSON.stringify(library.name)} returned HTTP ${confirmation.status}`);
      }
      confirmResourceSnapshot("script", body, confirmation.body);
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
    // The bearer captured at open() is NOT refreshed here, deliberately.
    // Measured 2026-09-14: the agent rotates on its own ~898s schedule and
    // `whoami --token` can hand back a token already near the end of that
    // cycle, so neither the file's age nor the token's tells you whether it is
    // still good — a prophylactic refresh would cost a CLI call per case and
    // still not guarantee one. `amRequest` instead refreshes and retries once
    // on a non-anonymous 401, which closes the gap completely for the price of
    // one wasted request on the rare occasion the bearer dies mid-file.
    // `/authenticate` is anonymous and carries no bearer, so a journey
    // invocation is unaffected either way.
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
    // No size guard: there is no script-specific limit, and the 5 MiB
    // request-body ceiling is orders of magnitude above any generated subject
    // (measured 2026-09-14, `docs/api/04-scripts.md`). Over it AM answers a
    // clean 400 and stores nothing, so the confirming read below is the only
    // check this needs — it never truncates.
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
      // A replaced minter script IS what the next invocation executes
      // (measured 2026-09-14): one file minted a session carrying "A", this
      // branch replaced the same script id to carry "B", and the subject
      // journey 1.1s later saw "B". AM does not serve a cached compile here,
      // so the slot is safe to reuse.
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
      request.managedFixtures,
      true
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
      if (kase.given.scriptName !== undefined && kase.given.scriptName !== `${this.identity.treeName}-subject`) {
        throw new AicLaneError(`${kase.name}: local scriptName differs from the uploaded subject name`);
      }
      if (kase.script !== this.#options.source) {
        throw new AicLaneError(`${kase.name}: case source differs from the leased suite source`);
      }
      if (!deepEqual(kase.given.libraries ?? {}, this.#options.libraries ?? {})) {
        throw new AicLaneError(`${kase.name}: case libraries differ from the leased suite libraries`);
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

  async #deleteCreatedResourcesSafely(): Promise<string[]> {
    const session = this.#session as TenantSession;
    const graph = this.#created.filter((resource) =>
      resource.kind !== "script" || !this.#ownedLibraries.has(resource.id));
    const errors = await deleteCreatedResources(this.#io, session, this.#realm, graph);
    const libraries = this.#created.filter((resource): resource is { kind: "script"; id: string } =>
      resource.kind === "script" && this.#ownedLibraries.has(resource.id));
    return [...errors, ...(await this.#cleanupOwnedLibraries(libraries.reverse(), this.#ownedLibraries, this.#realm))];
  }

  async #cleanupOwnedLibraries(
    libraries: readonly { kind: "script"; id: string }[],
    ownership: ReadonlyMap<string, OwnedLibrary>,
    realm: string
  ): Promise<string[]> {
    const errors: string[] = [];
    const ready: Array<{ kind: "script"; id: string }> = [];
    // Blank all owned sources before deleting any library. This removes
    // references among owned libraries, including references AM may count in
    // comments or in the library's own source. A confirming GET makes a lost
    // PUT response safe to replay from the journal on the next open.
    for (const resource of libraries) {
      const owned = ownership.get(resource.id);
      if (owned === undefined) {
        errors.push(`library ${resource.id} remains as residue: ownership metadata is missing`);
        continue;
      }
      if (owned.status === "not-owned") {
        errors.push(collisionResidue(owned));
        continue;
      }
      if (owned.status !== "owned") {
        errors.push(ambiguousLibraryResidue(owned, "unprobed"));
        continue;
      }
      const failure = await this.#tryBlankOwnedLibrary(resource, owned, realm);
      if (failure === undefined) {
        ready.push(resource);
      } else {
        errors.push(`library ${JSON.stringify(owned.name)} (${resource.id}) remains as residue: ${failure}`);
      }
    }
    let pending = ready;
    while (pending.length > 0) {
      const remaining: typeof pending = [];
      const failures = new Map<string, string>();
      for (const resource of pending) {
        const failure = await this.#tryDeleteBlankedOwnedLibrary(
          resource, ownership.get(resource.id) as OwnedLibrary, realm
        );
        if (failure !== undefined) {
          remaining.push(resource);
          failures.set(resource.id, failure);
        }
      }
      if (remaining.length === pending.length) {
        for (const resource of remaining) {
          const name = ownership.get(resource.id)?.name ?? resource.id;
          errors.push(`library ${JSON.stringify(name)} (${resource.id}) remains as residue: ${failures.get(resource.id)}`);
        }
        break;
      }
      pending = remaining;
    }
    return errors;
  }

  async #tryDeleteBlankedOwnedLibrary(
    resource: { kind: "script"; id: string },
    owned: OwnedLibrary,
    realm: string
  ): Promise<string | undefined> {
    const session = this.#session as TenantSession;
    try {
      const response = await amRequest(this.#io, session, {
        method: "GET", path: resourcePath(realm, resource), headers: amConfigHeaders(),
      });
      if (response.status === 404) return undefined;
      if (response.status !== 200) {
        return `could not check before delete (HTTP ${response.status})`;
      }
      const current = response.body as Record<string, unknown>;
      if (current?.name !== owned.name || current.context !== "LIBRARY" ||
          current.description !== owned.marker || current.script !== "") {
        return "warning: source or ownership fields changed after blanking; left in place";
      }
      return (await deleteCreatedResources(this.#io, session, realm, [resource]))[0];
    } catch (error) {
      return `could not check before delete: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  async #tryBlankOwnedLibrary(
    resource: { kind: "script"; id: string },
    owned: OwnedLibrary,
    realm: string
  ): Promise<string | undefined> {
    const session = this.#session as TenantSession;
    const path = resourcePath(realm, resource);
    try {
      const response = await amRequest(this.#io, session, {
        method: "GET", path, headers: amConfigHeaders(),
      });
      if (response.status === 404) return undefined;
      if (response.status !== 200) {
        return `could not check before blanking (HTTP ${response.status})`;
      }
      const current = response.body as Record<string, unknown>;
      const source = typeof current?.script === "string"
        ? Buffer.from(current.script, "base64").toString("utf8")
        : undefined;
      if (current?.name !== owned.name || current.context !== "LIBRARY" ||
          current.description !== owned.marker || source === undefined ||
          (sha256(source) !== owned.sourceHash && source !== "")) {
        return "warning: source or ownership fields changed after the lease created it; left in place";
      }
      if (source === "") return undefined;
      const blank = {
        _id: resource.id, name: owned.name, description: owned.marker,
        script: "", default: false, language: "JAVASCRIPT",
        context: "LIBRARY", evaluatorVersion: "2.0",
      };
      const put = await amRequest(this.#io, session, {
        method: "PUT", path, headers: amConfigHeaders(), body: JSON.stringify(blank),
      });
      if (put.status !== 200) {
        return `blank library source returned HTTP ${put.status}, expected 200`;
      }
      const confirmation = await amRequest(this.#io, session, {
        method: "GET", path, headers: amConfigHeaders(),
      });
      if (confirmation.status !== 200) {
        return `confirm blank library source returned HTTP ${confirmation.status}`;
      }
      confirmResourceSnapshot("script", blank, confirmation.body);
      return undefined;
    } catch (error) {
      return `could not blank and confirm: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  async #cleanupAfterFailedOpen(): Promise<string[]> {
    try {
      const errors = this.#session !== undefined && this.#created.length > 0
        ? await this.#deleteCreatedResourcesSafely()
        : [];
      // An older journal is never ours to erase here. Once any PUT was sent,
      // a lost response can leave residue absent from #created; retain the
      // journal for the next open to probe even if known deletes succeeded.
      if (this.#journalWrittenForOpen && !this.#writeAttempted && this.#paths !== undefined) {
        await removeLeaseJournal(this.#paths.journalPath);
      }
      return errors;
    } finally {
      await this.#lock?.release();
    }
  }

  async #handleOlderJournal(): Promise<void> {
    const paths = this.#paths as LeaseStatePaths;
    const journal = await readLeaseJournal(paths.journalPath);
    if (journal === undefined) {
      return;
    }
    const residue: string[] = [];
    const ownership = new Map((journal.ownedLibraries ?? []).map((item) => [item.id, item]));
    const libraries: Array<{ kind: "script"; id: string }> = [];
    for (const resource of journal.resources) {
      if (resource.kind === "script" && ownership.has(resource.id)) {
        const owned = ownership.get(resource.id) as OwnedLibrary;
        if (owned.status === "not-owned") {
          residue.push(collisionResidue(owned));
          continue;
        }
        if (owned.status !== "owned") {
          const response = await amRequest(this.#io, this.#session as TenantSession, {
            method: "GET", path: resourcePath(journal.realm, resource),
            headers: amConfigHeaders(),
          });
          if (response.status !== 200 && response.status !== 404) {
            throw new AicLaneError(`could not probe older library ${JSON.stringify(owned.name)} (HTTP ${response.status})`);
          }
          residue.push(ambiguousLibraryResidue(owned,
            response.status === 200 ? "present" : "absent"));
          continue;
        }
        const marker = parseLeaseMarker(owned.marker);
        if (resource.id !== uuidV5(`${journal.aicId}:library:${owned.name}`) ||
            marker?.idHash !== sha256(journal.aicId).slice(0, 20) ||
            marker.ownerToken !== journal.ownerToken ||
            (marker.id !== undefined && marker.id !== journal.aicId)) {
          residue.push(`library ${JSON.stringify(owned.name)} (${resource.id}) has invalid journal ownership`);
        } else {
          libraries.push(resource);
        }
        continue;
      }
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
    residue.push(...(await this.#cleanupOwnedLibraries(libraries.reverse(), ownership, journal.realm)));
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
  // The kind goes in the message and the original goes on `cause`: the message
  // must not carry arbitrary assertion text, but a remote check reporting only
  // "threw Error" tells you nothing about what it actually found.
  return new AicLaneHookError(
    `local and AIC lanes disagreed for ${JSON.stringify(name)}: local ${hook} passed; AIC ${hook} threw ${safeErrorKind(error)}`,
    { cause: error }
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

function collisionResidue(library: OwnedLibrary): string {
  return `library ${JSON.stringify(library.name)} (${library.id}) remains as residue after a create collision: its source was overwritten by the lease; operator inspection is required and automatic cleanup is disabled`;
}

function ambiguousLibraryResidue(library: OwnedLibrary, presence: string): string {
  return `library ${JSON.stringify(library.name)} (${library.id}) has a legacy journal entry without ownership status (${presence}); operator inspection is required and automatic cleanup or recreation is disabled`;
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
