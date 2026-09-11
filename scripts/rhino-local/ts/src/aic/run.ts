import { randomUUID } from "node:crypto";
import type { Case, RecordedEffects } from "../case/types.ts";
import { repoRoot } from "../paths.ts";
import { parseAuthenticateCallbacks } from "./callbacks.ts";
import { emitWrapperJourney, type WrapperJourney } from "./emit-journey.ts";
import { assembleEffects, parseSubjectDump } from "./record.ts";
import {
  AicLaneError,
  amRequest,
  connectTenant,
  defaultAicIo,
  realmJsonPath,
  type AicIo,
  type AmResponse,
  type TenantSession,
} from "./tenant.ts";
import { aicUnsupportedReason } from "./unsupported.ts";

export interface RunAicOptions {
  io?: AicIo;
  tenant?: string;
  realm?: string;
  runId?: string;
  project?: string;
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
  const reason = aicUnsupportedReason(kase);
  if (reason !== undefined) {
    throw new AicLaneError(`AIC lane skipped: ${reason}`);
  }
  const project = options.project ?? repoRoot;
  const io = options.io ?? defaultAicIo(project);
  const session = await connectTenant(io, {
    ...(options.tenant !== undefined ? { tenant: options.tenant } : {}),
    project,
  });
  const wrapper = emitWrapperJourney(kase, source, {
    ...(options.runId !== undefined ? { runId: options.runId } : { runId: randomUUID().replace(/-/g, "").slice(0, 12) }),
    ...(options.realm !== undefined ? { realm: options.realm } : {}),
  });
  const created: Created[] = [];
  try {
    await provision(io, session, wrapper, created);
    const authenticate = await invoke(io, session, wrapper);
    return recordFromAuthenticate(kase, authenticate);
  } finally {
    await cleanup(io, session, wrapper.realm, created);
  }
}

async function provision(
  io: AicIo,
  session: TenantSession,
  wrapper: WrapperJourney,
  created: Created[]
): Promise<void> {
  const base = realmJsonPath(wrapper.realm);
  await refuseIfExists(
    io,
    session,
    "GET",
    `${base}/realm-config/authentication/authenticationtrees/trees/${encodeURIComponent(wrapper.treeName)}`,
    `tree ${wrapper.treeName}`
  );
  for (const script of wrapper.scripts) {
    await refuseIfExists(io, session, "GET", `${base}/scripts/${script.id}`, `script ${script.id}`);
  }
  for (const script of wrapper.scripts) {
    const body = JSON.stringify({
      _id: script.id,
      name: script.name,
      description: "rhino-local AIC lane throwaway. Safe to delete.",
      script: Buffer.from(script.source, "utf8").toString("base64"),
      default: false,
      language: "JAVASCRIPT",
      context: "AUTHENTICATION_TREE_DECISION_NODE",
      evaluatorVersion: "2.0",
    });
    const response = await amRequest(io, session, {
      method: "PUT",
      path: `${base}/scripts/${script.id}`,
      body,
    });
    expectOk(response, `PUT script ${script.name}`);
    created.push({ kind: "script", id: script.id });
  }
  for (const node of wrapper.nodes) {
    const nodeBody = wrapper.nodeBodies[node.id];
    if (nodeBody === undefined) {
      throw new AicLaneError(`missing node body for ${node.id}`);
    }
    const path = `${base}/realm-config/authentication/authenticationtrees/nodes/ScriptedDecisionNode/${node.id}`;
    await refuseIfExists(io, session, "GET", path, `node ${node.id}`);
    const response = await amRequest(io, session, {
      method: "PUT",
      path,
      body: JSON.stringify(nodeBody),
    });
    expectOk(response, `PUT node ${node.displayName}`);
    created.push({ kind: "node", id: node.id });
  }
  const treeResponse = await amRequest(io, session, {
    method: "PUT",
    path: `${base}/realm-config/authentication/authenticationtrees/trees/${encodeURIComponent(wrapper.treeName)}`,
    body: JSON.stringify(wrapper.treeBody),
  });
  expectOk(treeResponse, `PUT tree ${wrapper.treeName}`);
  created.push({ kind: "tree", name: wrapper.treeName });
}

async function invoke(
  io: AicIo,
  session: TenantSession,
  wrapper: WrapperJourney
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
    body: "{}",
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
  created: Created[]
): Promise<void> {
  const base = realmJsonPath(realm);
  const errors: string[] = [];
  for (const item of created.slice().reverse()) {
    try {
      if (item.kind === "tree") {
        await amRequest(io, session, {
          method: "DELETE",
          path: `${base}/realm-config/authentication/authenticationtrees/trees/${encodeURIComponent(item.name)}`,
        });
      } else if (item.kind === "node") {
        await amRequest(io, session, {
          method: "DELETE",
          path: `${base}/realm-config/authentication/authenticationtrees/nodes/ScriptedDecisionNode/${item.id}`,
        });
      } else {
        await amRequest(io, session, {
          method: "DELETE",
          path: `${base}/scripts/${item.id}`,
        });
      }
    } catch (error) {
      errors.push(error instanceof Error ? error.message : String(error));
    }
  }
  if (errors.length > 0) {
    // Cleanup is best-effort; leftover throwaways are namespaced rl-aic-*.
    // Surface the failures without hiding the run result (this is finally).
    process.emitWarning(`rhino-local AIC cleanup: ${errors.join("; ")}`);
  }
}

async function refuseIfExists(
  io: AicIo,
  session: TenantSession,
  method: string,
  path: string,
  label: string
): Promise<void> {
  const response = await amRequest(io, session, { method, path });
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

function expectOk(response: AmResponse, label: string): void {
  if (response.status >= 200 && response.status < 300) {
    return;
  }
  throw new AicLaneError(
    `${label} HTTP ${response.status}${txid(response)}: ${snippet(response)}`,
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
    lower === "connection"
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
