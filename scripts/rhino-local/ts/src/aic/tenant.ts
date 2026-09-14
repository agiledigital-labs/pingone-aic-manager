import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { join } from "node:path";
import { isPlainObject } from "../case/util.ts";
import { repoRoot } from "../paths.ts";
import { headerValues, sendHttp, type HttpRequest, type HttpResponse } from "./http.ts";

const execFileAsync = promisify(execFile);

export interface TenantSession {
  tenantName: string;
  baseUrl: string;
  token: string;
  project: string;
}

export interface CliResult {
  status: number;
  stdout: string;
  stderr: string;
}

export interface AmResponse {
  status: number;
  bodyText: string;
  body: unknown;
  transactionId?: string;
}

export interface AmRequest {
  method: string;
  path: string;
  headers?: Array<[string, string]>;
  body?: string;
  /** When true, do not send the service-account bearer (authenticate). */
  anonymous?: boolean;
  timeoutMs?: number;
}

export interface AicIo {
  aic(args: string[]): Promise<CliResult>;
  http(req: HttpRequest): Promise<HttpResponse>;
}

export const AM_CONFIG_API_VERSION = "protocol=2.0,resource=1.0";

export function amConfigHeaders(): Array<[string, string]> {
  return [["Accept-API-Version", AM_CONFIG_API_VERSION]];
}

export class AicLaneError extends Error {
  readonly status?: number;
  readonly transactionId?: string;

  constructor(
    message: string,
    options: { status?: number; transactionId?: string; cause?: unknown } = {}
  ) {
    super(message, options.cause !== undefined ? { cause: options.cause } : {});
    this.name = "AicLaneError";
    if (options.status !== undefined) {
      this.status = options.status;
    }
    if (options.transactionId !== undefined) {
      this.transactionId = options.transactionId;
    }
  }
}

export function defaultAicIo(project: string): AicIo {
  const bin = process.env.AIC_BIN ?? join(project, "target", "debug", "aic");
  return {
    async aic(args) {
      try {
        const result = await execFileAsync(bin, args, {
          cwd: project,
          timeout: 30_000,
          encoding: "utf8",
          maxBuffer: 2_000_000,
        });
        return { status: 0, stdout: result.stdout, stderr: result.stderr };
      } catch (error) {
        if (isExecError(error)) {
          return {
            status: error.code === null || error.code === undefined ? 1 : execStatus(error.code),
            stdout: error.stdout ?? "",
            stderr: error.stderr ?? error.message,
          };
        }
        throw error;
      }
    },
    http: sendHttp,
  };
}

function isExecError(
  error: unknown
): error is Error & { code?: string | number | null; stdout?: string; stderr?: string } {
  return error instanceof Error;
}

function execStatus(code: string | number): number {
  return typeof code === "number" ? code : 1;
}

export async function connectTenant(
  io: AicIo,
  options: { tenant?: string; project?: string } = {}
): Promise<TenantSession> {
  const project = options.project ?? repoRoot;
  const noPrompt = ["--no-prompt", "--project", project];
  const list = await io.aic([...noPrompt, "ctx", "list", "--json"]);
  if (list.status !== 0) {
    throw new AicLaneError(
      `aic ctx list failed (status ${list.status}): ${trim(list.stderr || list.stdout)}`
    );
  }
  const tenants = parseCtxList(list.stdout);
  const chosen =
    options.tenant !== undefined
      ? tenants.find((row) => row.name === options.tenant)
      : tenants.find((row) => row.current) ?? tenants[0];
  if (chosen === undefined) {
    throw new AicLaneError(
      options.tenant === undefined
        ? "aic ctx list returned no tenants"
        : `aic ctx list has no tenant named ${JSON.stringify(options.tenant)}`
    );
  }
  const whoamiArgs = [...noPrompt, "whoami", "--token"];
  if (options.tenant !== undefined) {
    whoamiArgs.push("--tenant", options.tenant);
  }
  const whoami = await io.aic(whoamiArgs);
  if (whoami.status !== 0) {
    throw new AicLaneError(
      `aic whoami --token failed (status ${whoami.status}): ${trim(whoami.stderr || whoami.stdout)}. Unlock the agent (aic login) or pass a reachable tenant.`
    );
  }
  const token = whoami.stdout.trim();
  if (token.length === 0) {
    throw new AicLaneError("aic whoami --token printed an empty token");
  }
  return {
    tenantName: chosen.name,
    baseUrl: stripSlash(chosen.base_url),
    token,
    project,
  };
}

interface CtxRow {
  current: boolean;
  name: string;
  base_url: string;
}

function parseCtxList(stdout: string): CtxRow[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new AicLaneError(`aic ctx list --json is not JSON: ${message}`);
  }
  if (!Array.isArray(parsed)) {
    throw new AicLaneError("aic ctx list --json is not an array");
  }
  const rows: CtxRow[] = [];
  for (const item of parsed) {
    if (!isPlainObject(item) || typeof item.name !== "string" || typeof item.base_url !== "string") {
      throw new AicLaneError("aic ctx list --json row is missing name/base_url");
    }
    rows.push({
      current: item.current === true,
      name: item.name,
      base_url: item.base_url,
    });
  }
  return rows;
}

/**
 * Re-ask the agent for this tenant's bearer and adopt it.
 *
 * MEASURED 2026-09-14 over 20 minutes: `aic whoami --token` handed back a
 * token that worked *right now* and said nothing about how much life was left
 * in it. The first token this probe was given was rejected 401 five minutes
 * later, while a token fetched moments after it stayed good for the next ten —
 * the agent rotated on its own ~898s schedule, so what you got depended on
 * where in that cycle you asked.
 *
 * `aic` now guarantees `--token` at least 840 of those seconds, minting when
 * the cached token is below the floor (MEASURED 2026-09-15: with 823s left the
 * TTL jumped to 899 across the call, and with 864s left it did not move). That
 * removes the reason this refresh usually fired, and none of the reasons it
 * must stay: a file may run longer than the floor, the tenant may revoke, and
 * an older `aic` on someone else's PATH makes no such promise. So the 401 retry
 * remains the correctness mechanism and the floor is only what makes it rare.
 */
export async function refreshSessionToken(
  io: AicIo,
  session: TenantSession
): Promise<void> {
  const args = ["--no-prompt", "--project", session.project, "whoami", "--token"];
  if (session.tenantName.length > 0) {
    args.push("--tenant", session.tenantName);
  }
  const whoami = await io.aic(args);
  if (whoami.status !== 0) {
    throw new AicLaneError(
      `aic whoami --token failed while refreshing the bearer (status ${whoami.status}): ${trim(whoami.stderr || whoami.stdout)}`
    );
  }
  const token = whoami.stdout.trim();
  if (token.length === 0) {
    throw new AicLaneError("aic whoami --token printed an empty token on refresh");
  }
  session.token = token;
}

export function realmJsonPath(realm: string): string {
  return `/am/json/realms/root/realms/${realm}`;
}

export async function amRequest(
  io: AicIo,
  session: TenantSession,
  req: AmRequest
): Promise<AmResponse> {
  const first = await sendAmRequest(io, session, req);
  // A 401 on an authenticated config call means the bearer died under us, not
  // that the request was wrong — so refresh once and retry. Anonymous calls
  // are excluded deliberately: `/authenticate` answers a journey that reached
  // no declared outcome with a bare 401, and that is a verdict, not an
  // expired token.
  if (first.status !== 401 || req.anonymous === true) {
    return first;
  }
  await refreshSessionToken(io, session);
  return sendAmRequest(io, session, req);
}

async function sendAmRequest(
  io: AicIo,
  session: TenantSession,
  req: AmRequest
): Promise<AmResponse> {
  const url = `${session.baseUrl}${req.path}`;
  const headers: Array<[string, string]> = [
    ["Accept", "application/json"],
    ["Content-Type", "application/json"],
    ...(req.headers ?? []),
  ];
  if (!req.anonymous) {
    headers.push(["Authorization", `Bearer ${session.token}`]);
    headers.push(["X-Requested-With", "XMLHttpRequest"]);
  }
  const response = await io.http({
    url,
    method: req.method,
    headerLines: headers,
    ...(req.body !== undefined ? { body: req.body } : {}),
    ...(req.timeoutMs !== undefined ? { timeoutMs: req.timeoutMs } : {}),
  });
  const transactionId = headerValues(response.headers, "x-forgerock-transactionid")[0];
  const bodyText = redact(response.body, session);
  let body: unknown = null;
  if (response.body.length > 0) {
    try {
      body = JSON.parse(response.body);
    } catch {
      body = bodyText;
    }
  }
  return {
    status: response.status,
    bodyText,
    body,
    ...(transactionId !== undefined ? { transactionId } : {}),
  };
}

export function redact(text: string, session: TenantSession): string {
  let out = text;
  const secrets = [session.token, session.baseUrl, hostnameOf(session.baseUrl)];
  for (const secret of secrets) {
    if (secret.length === 0) {
      continue;
    }
    out = out.split(secret).join("<redacted>");
  }
  return out;
}

function hostnameOf(baseUrl: string): string {
  try {
    return new URL(baseUrl).hostname;
  } catch {
    return "";
  }
}

function stripSlash(url: string): string {
  return url.endsWith("/") ? url.slice(0, -1) : url;
}

function trim(text: string): string {
  const sliced = text.trim().replace(/\s+/g, " ");
  return sliced.length > 400 ? `${sliced.slice(0, 400)}…` : sliced;
}
