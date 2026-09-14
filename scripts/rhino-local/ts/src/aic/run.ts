import { randomUUID } from "node:crypto";
import type { Case, JsonValue, RecordedEffects } from "../case/types.ts";
import { repoRoot } from "../paths.ts";
import { fillCallbackInputs, parseAuthenticateCallbacks } from "./callbacks.ts";
import { emitWrapperJourney, type WrapperJourney } from "./emit-journey.ts";
import { emitSessionJourney } from "./emit-session.ts";
import {
  acquireManagedFixtureLock,
  deleteManagedFixture,
  managedSeedMatches,
  seedManagedFixtures,
  type ManagedFixture,
  type SeededManagedFixture,
} from "./managed.ts";
import { assembleEffects, parseSubjectDump } from "./record.ts";
import {
  confirmResourceSnapshot,
  resourceRequestProjection,
  type AicResourceKind,
} from "./resource-snapshot.ts";
import {
  AicLaneError,
  amConfigHeaders,
  amRequest,
  connectTenant,
  defaultAicIo,
  realmJsonPath,
  type AicIo,
  type AmResponse,
  type TenantSession,
} from "./tenant.ts";
import { beginSingleTrace, beginTrace, clearAicTrace } from "./trace.ts";
import { TX_HEADER } from "./txid.ts";
import { aicUnsupportedReason } from "./unsupported.ts";

/** One submitted callback value, matched to an emitted callback by type. */
export interface AicReply {
  type: string;
  value: JsonValue;
}

export interface RunAicOptions {
  io?: AicIo;
  tenant?: string;
  realm?: string;
  runId?: string;
  project?: string;
  /** Harness-owned records from the local lease's fixture ledger. */
  managedFixtures?: readonly ManagedFixture[];
  /**
   * One entry per suspended pass, in order: what the client submits to advance
   * the journey. Omit for a single-pass case. The journey is provisioned once
   * and every pass re-enters the same node, which is what makes this the
   * counterpart of the local lane's `.step()` chain rather than N runs.
   */
  replies?: readonly (readonly AicReply[])[];
}

type Created =
  | { kind: "script"; id: string }
  | { kind: "node"; id: string }
  | { kind: "tree"; name: string };

/**
 * Create a namespaced wrapper journey, invoke it once, record effects, and
 * delete everything created. Refuses to overwrite an existing script, node,
 * or tree. Every `aic` invocation goes through `--no-prompt`.
 */
export async function runAicLane(
  kase: Case,
  source: string,
  options: RunAicOptions = {}
): Promise<RecordedEffects> {
  const passes = await runAicChain([kase], source, options);
  return passes[passes.length - 1] as RecordedEffects;
}

/**
 * Run a step chain on a tenant: one journey, one node, entered once per case.
 *
 * `cases` is one `Case` per pass — `cases[0].given` seeds the journey and every
 * later `given` is what the local lane computed for that pass. Handing the
 * carried seeds over rather than recomputing them here is the whole point of
 * the two lanes: the tenant's own `before` snapshot is checked against them
 * (`verifySeedsVisible`), so a carry rule that is wrong — carrying transient
 * state across a suspend, say — fails loudly here instead of agreeing with
 * itself locally.
 *
 * Managed state is accepted only with a fixture ledger that exactly matches
 * the first pass. Those records are create-only seeded, read back, and removed
 * around the whole journey while a per-tenant lock excludes parallel fixture
 * runs. Later passes may carry managed changes made by the subject itself.
 *
 * `options.replies` must hold one entry per suspended pass, so
 * `replies.length === cases.length - 1`.
 */
export async function runAicChain(
  cases: readonly Case[],
  source: string,
  options: RunAicOptions = {}
): Promise<RecordedEffects[]> {
  const first = cases[0];
  const last = cases[cases.length - 1];
  if (first === undefined || last === undefined) {
    throw new AicLaneError("runAicChain needs at least one case");
  }
  const replies = options.replies ?? [];
  if (replies.length !== cases.length - 1) {
    throw new AicLaneError(
      `runAicChain: ${cases.length} passes need ${cases.length - 1} reply sets, got ${replies.length}`
    );
  }
  const managedFixtures = options.managedFixtures ?? [];
  const harnessOwnsManaged =
    options.managedFixtures !== undefined &&
    managedSeedMatches(first.given.managed, managedFixtures);
  if (options.managedFixtures !== undefined && !harnessOwnsManaged) {
    throw new AicLaneError(
      "managed fixture provenance does not match the first pass's given.managed seed"
    );
  }
  for (const kase of cases) {
    const reason = aicUnsupportedReason(kase, { harnessOwnsManaged });
    if (reason !== undefined) {
      throw new AicLaneError(`AIC lane skipped: ${reason}`);
    }
  }
  const kase = emitCase(cases);
  const project = options.project ?? repoRoot;
  const io = options.io ?? defaultAicIo(project);
  const session = await connectTenant(io, {
    ...(options.tenant !== undefined ? { tenant: options.tenant } : {}),
    project,
  });
  const runId =
    options.runId ?? randomUUID().replace(/-/g, "").slice(0, 12);
  const wrapper = emitWrapperJourney(kase, source, {
    runId,
    ...(options.realm !== undefined ? { realm: options.realm } : {}),
  });
  const created: Created[] = [];
  const seededManaged: SeededManagedFixture[] = [];
  const releaseManagedLock =
    managedFixtures.length === 0
      ? async (): Promise<void> => Promise.resolve()
      : await acquireManagedFixtureLock(session, last.name);
  try {
    await seedManagedFixtures(io, session, managedFixtures, seededManaged);
    if (kase.given.existingSession !== undefined) {
      await mintSession(
        io,
        session,
        kase.given.existingSession,
        wrapper,
        runId,
        created
      );
      // Mint succeeded: its one-off id is harness plumbing, not a subject pass.
      // A mint *failure* leaves that trace in place so show-log can fetch it.
      clearAicTrace();
    }
    await provision(io, session, wrapper, created);
    return await drive(io, session, wrapper, cases, replies, beginTrace(session.tenantName));
  } finally {
    try {
      await cleanup(io, session, wrapper.realm, created, seededManaged);
    } finally {
      await releaseManagedLock();
    }
  }
}

/**
 * Run a throwaway journey to completion and put its session cookie on the
 * subject's invoke. This is the only way to seed `existingSession`: a session
 * exists only after a journey completes (`docs/api/09-journeys.md`).
 *
 * The mini journey is provisioned into the same `created` list as the subject,
 * so one cleanup removes both even if the subject invoke throws.
 *
 * Its authenticate call is *not* a numbered pass of the subject stem: pass 1
 * of the script under test must stay `-01`. Minting gets its own uuid so a
 * mint failure is still look-up-able; a successful mint is cleared before the
 * subject runs.
 */
async function mintSession(
  io: AicIo,
  session: TenantSession,
  existingSession: Record<string, string>,
  wrapper: WrapperJourney,
  runId: string,
  created: Created[]
): Promise<void> {
  const minter = emitSessionJourney(existingSession, {
    runId,
    realm: wrapper.realm,
  });
  await provision(io, session, minter, created);
  const response = await invoke(
    io,
    session,
    minter,
    beginSingleTrace(session.tenantName)
  );
  const body = response.body as { tokenId?: unknown };
  if (typeof body.tokenId !== "string" || body.tokenId.length === 0) {
    // A journey that returns callbacks has not completed, so there is no
    // session yet; seeding nothing and running anyway would grade the subject
    // as though the harness had meant the binding to be absent.
    throw new AicLaneError(
      `session-minting journey ${minter.treeName} returned no tokenId${txid(response)}: ${snippet(response)}`
    );
  }
  const cookieName = await fetchCookieName(io, session);
  wrapper.invoke.cookies[cookieName] = body.tokenId;
}

/**
 * The session cookie's name is per-tenant, so it has to be read rather than
 * assumed (`given.cookieName` is refused on this lane for the same reason).
 */
async function fetchCookieName(
  io: AicIo,
  session: TenantSession
): Promise<string> {
  const response = await amRequest(io, session, {
    method: "GET",
    path: "/am/json/serverinfo/*",
    anonymous: true,
  });
  const name = (response.body as { cookieName?: unknown }).cookieName;
  if (typeof name !== "string" || name.length === 0) {
    throw new AicLaneError(
      `GET /am/json/serverinfo/* returned no cookieName (HTTP ${response.status})`
    );
  }
  return name;
}

async function provision(
  io: AicIo,
  session: TenantSession,
  wrapper: WrapperJourney,
  created: Created[]
): Promise<void> {
  const base = realmJsonPath(wrapper.realm);
  const resources: Array<{
    kind: AicResourceKind;
    path: string;
    label: string;
    body: Record<string, unknown>;
    created: Created;
  }> = [];
  for (const script of wrapper.scripts) {
    resources.push({
      kind: "script",
      path: `${base}/scripts/${script.id}`,
      label: `script ${script.name}`,
      created: { kind: "script", id: script.id },
      body: {
      _id: script.id,
      name: script.name,
      description: "rhino-local AIC lane throwaway. Safe to delete.",
      script: Buffer.from(script.source, "utf8").toString("base64"),
      default: false,
      language: "JAVASCRIPT",
      context: "AUTHENTICATION_TREE_DECISION_NODE",
      evaluatorVersion: "2.0",
      },
    });
  }
  for (const node of wrapper.nodes) {
    const nodeBody = wrapper.nodeBodies[node.id];
    if (nodeBody === undefined) {
      throw new AicLaneError(`missing node body for ${node.id}`);
    }
    resources.push({
      kind: "node",
      path: `${base}/realm-config/authentication/authenticationtrees/nodes/ScriptedDecisionNode/${node.id}`,
      label: `node ${node.displayName}`,
      created: { kind: "node", id: node.id },
      body: nodeBody,
    });
  }
  resources.push({
    kind: "tree",
    path: `${base}/realm-config/authentication/authenticationtrees/trees/${encodeURIComponent(wrapper.treeName)}`,
    label: `tree ${wrapper.treeName}`,
    created: { kind: "tree", name: wrapper.treeName },
    body: wrapper.treeBody,
  });
  for (const resource of resources) {
    await refuseIfExists(io, session, resource.path, resource.label);
  }
  for (const resource of resources) {
    const submitted = resourceRequestProjection(resource.kind, resource.body);
    const response = await amRequest(io, session, {
      method: "PUT",
      path: resource.path,
      headers: amConfigHeaders(),
      body: JSON.stringify(submitted),
    });
    if (response.status !== 201) {
      expectStatus(response, `PUT ${resource.label}`, 201);
    }
    created.push(resource.created);
    const confirmation = await amRequest(io, session, {
      method: "GET",
      path: resource.path,
      headers: amConfigHeaders(),
    });
    expectStatus(confirmation, `confirm ${resource.label}`, 200);
    confirmResourceSnapshot(resource.kind, submitted, confirmation.body);
  }
}

/**
 * Invoke the journey, then answer each declared pass in turn.
 *
 * Every pass re-enters the same node, so the tenant carries state between them
 * on its own terms — which is the point: the local lane has to model that, and
 * this is what it is modelled against. A pass that reaches the result node
 * while replies are still pending is an error, because the chain then asserts
 * against a journey shorter than it described.
 */
async function drive(
  io: AicIo,
  session: TenantSession,
  wrapper: WrapperJourney,
  cases: readonly Case[],
  replies: readonly (readonly AicReply[])[],
  tx: { next: () => string }
): Promise<RecordedEffects[]> {
  const passes: RecordedEffects[] = [];
  let response = await invoke(io, session, wrapper, tx.next());
  for (const [index, reply] of replies.entries()) {
    const kase = cases[index] as Case;
    const label = kase.name;
    const parsed = parseAuthenticateCallbacks(response.body);
    if (parsed.dumpRaw !== undefined) {
      throw new AicLaneError(
        `${label}: the journey finished before this step ran — the script decided an outcome instead of sending callbacks`
      );
    }
    passes.push(
      assembleEffects({ given: kase.given, callbacks: parsed.callbacks })
    );
    const body = fillCallbackInputs(response.body, reply, label);
    response = await invoke(io, session, wrapper, tx.next(), body);
  }
  passes.push(recordFromAuthenticate(cases[cases.length - 1] as Case, response));
  return passes;
}

/**
 * The case the wrapper journey is emitted from: the first pass's seed, with
 * every outcome any pass may reach declared on the subject node. A tenant
 * answers an undeclared outcome with a bare 401, so a chain whose last pass
 * decides something the first never mentions has to say so up front.
 */
function emitCase(cases: readonly Case[]): Case {
  const first = cases[0] as Case;
  const outcomes = new Set<string>(first.outcomes ?? []);
  for (const kase of cases) {
    if (kase.expect.outcome !== null) {
      outcomes.add(kase.expect.outcome);
    }
  }
  return outcomes.size === 0 ? first : { ...first, outcomes: [...outcomes] };
}

async function invoke(
  io: AicIo,
  session: TenantSession,
  wrapper: WrapperJourney,
  transactionId: string,
  body = "{}"
): Promise<AmResponse> {
  const url = new URL(
    `${session.baseUrl}${realmJsonPath(wrapper.realm)}/authenticate`
  );
  url.searchParams.append("authIndexType", "service");
  url.searchParams.append("authIndexValue", wrapper.treeName);
  for (const [key, values] of Object.entries(wrapper.invoke.parameters)) {
    for (const value of values) {
      url.searchParams.append(key, value);
    }
  }
  const headers: Array<[string, string]> = [
    ["Accept-API-Version", "protocol=1.0,resource=2.1"],
    [TX_HEADER, transactionId],
  ];
  for (const [key, values] of Object.entries(wrapper.invoke.headers)) {
    if (isReservedInvokeHeader(key)) {
      continue;
    }
    for (const value of values) {
      headers.push([key, value]);
    }
  }
  const cookie = cookieHeader(wrapper.invoke.cookies);
  if (cookie !== undefined) {
    headers.push(["Cookie", cookie]);
  }
  const pathAndQuery = `${url.pathname}${url.search}`;
  const response = await amRequest(io, session, {
    method: "POST",
    path: pathAndQuery,
    headers,
    body,
    anonymous: true,
  });
  if (response.status < 200 || response.status >= 300) {
    throw new AicLaneError(
      `authenticate HTTP ${response.status}${txid(response)}: ${snippet(response)}`,
      {
        ...(response.status !== undefined ? { status: response.status } : {}),
        ...(response.transactionId !== undefined
          ? { transactionId: response.transactionId }
          : {}),
      }
    );
  }
  return response;
}

function recordFromAuthenticate(kase: Case, response: AmResponse): RecordedEffects {
  const parsed = parseAuthenticateCallbacks(response.body);
  if (parsed.dumpRaw !== undefined) {
    return assembleEffects({
      given: kase.given,
      dump: parseSubjectDump(parsed.dumpRaw),
      callbacks: parsed.callbacks,
    });
  }
  if (parsed.callbacks.length > 0) {
    return assembleEffects({
      given: kase.given,
      callbacks: parsed.callbacks,
    });
  }
  throw new AicLaneError(
    `authenticate returned HTTP ${response.status} with no harness dump and no callbacks${txid(response)}: ${snippet(response)}`
  );
}

async function cleanup(
  io: AicIo,
  session: TenantSession,
  realm: string,
  created: Created[],
  seededManaged: readonly SeededManagedFixture[]
): Promise<void> {
  const base = realmJsonPath(realm);
  const errors: string[] = [];
  for (const item of created.slice().reverse()) {
    try {
      let response: AmResponse;
      if (item.kind === "tree") {
        response = await amRequest(io, session, {
          method: "DELETE",
          path: `${base}/realm-config/authentication/authenticationtrees/trees/${encodeURIComponent(item.name)}`,
          headers: amConfigHeaders(),
        });
      } else if (item.kind === "node") {
        response = await amRequest(io, session, {
          method: "DELETE",
          path: `${base}/realm-config/authentication/authenticationtrees/nodes/ScriptedDecisionNode/${item.id}`,
          headers: amConfigHeaders(),
        });
      } else {
        response = await amRequest(io, session, {
          method: "DELETE",
          path: `${base}/scripts/${item.id}`,
          headers: amConfigHeaders(),
        });
      }
      expectDeleted(response, `AIC ${item.kind}`);
    } catch (error) {
      errors.push(error instanceof Error ? error.message : String(error));
    }
  }
  for (const fixture of seededManaged.slice().reverse()) {
    try {
      await deleteManagedFixture(io, session, fixture);
    } catch (error) {
      errors.push(error instanceof Error ? error.message : String(error));
    }
  }
  if (errors.length > 0) {
    // Cleanup is best-effort so one failed delete does not prevent the rest.
    // Surface every leak without hiding the subject failure (this is finally).
    process.emitWarning(`rhino-local AIC cleanup: ${errors.join("; ")}`);
  }
}

function expectDeleted(response: AmResponse, label: string): void {
  if (
    response.status === 404 ||
    (response.status >= 200 && response.status < 300)
  ) {
    return;
  }
  throw new AicLaneError(
    `delete ${label} returned HTTP ${response.status}: ${snippet(response)}`
  );
}

async function refuseIfExists(
  io: AicIo,
  session: TenantSession,
  path: string,
  label: string
): Promise<void> {
  const response = await amRequest(io, session, {
    method: "GET",
    path,
    headers: amConfigHeaders(),
  });
  if (response.status === 404) {
    return;
  }
  if (response.status >= 200 && response.status < 300) {
    throw new AicLaneError(
      `refusing to overwrite existing ${label} (GET ${path} returned ${response.status})`
    );
  }
  throw new AicLaneError(
    `GET ${label} HTTP ${response.status}: ${snippet(response)}`,
    { status: response.status }
  );
}

function expectStatus(response: AmResponse, label: string, status: number): void {
  if (response.status === status) {
    return;
  }
  throw new AicLaneError(
    `${label} returned HTTP ${response.status}, expected ${status}${txid(response)}: ${snippet(response)}`,
    {
      status: response.status,
      ...(response.transactionId !== undefined
        ? { transactionId: response.transactionId }
        : {}),
    }
  );
}

function isReservedInvokeHeader(name: string): boolean {
  const lower = name.toLowerCase();
  return (
    lower === "host" ||
    lower === "content-length" ||
    lower === "transfer-encoding" ||
    lower === "connection" ||
    lower === TX_HEADER
  );
}

function cookieHeader(cookies: Record<string, string>): string | undefined {
  const parts: string[] = [];
  for (const [name, value] of Object.entries(cookies)) {
    parts.push(`${name}=${value}`);
  }
  return parts.length === 0 ? undefined : parts.join("; ");
}

function txid(response: AmResponse): string {
  return response.transactionId === undefined ? "" : ` tx=${response.transactionId}`;
}

function snippet(response: AmResponse): string {
  const trimmed = response.bodyText.replace(/\s+/g, " ").trim();
  return trimmed.length > 300 ? `${trimmed.slice(0, 300)}…` : trimmed;
}
