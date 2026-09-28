import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { join } from "node:path";
import { isPlainObject } from "../case/util.ts";
import { projectRoot } from "../project.ts";
import { headerValues, sendHttp, type HttpRequest, type HttpResponse } from "./http.ts";
import {
  configuredTenantProvider,
  TENANT_ENV,
  type LogReader,
  type TenantDescription,
  type TenantProvider,
} from "./provider.ts";

const execFileAsync = promisify(execFile);

export interface TenantSession {
  tenantName: string;
  baseUrl: string;
  token: string;
  /** Where `token` came from, and where a replacement comes from after a 401. */
  provider: TenantProvider;
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
        if (isExecError(error) && error.code === "ENOENT") {
          throw new AicLaneError(
            `no tenant provider configured: pass \`provider\`, call setTenantProvider(), ` +
              `or set ${TENANT_ENV.url} (see docs/rhino-local-harness.md); ` +
              `the aic fallback needs the aic CLI, and ${bin} does not exist (set AIC_BIN)`,
            { cause: error }
          );
        }
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

export interface ConnectOptions {
  /**
   * An `aic` context name. With the `aic` provider it selects the context;
   * with any other it must equal the provider's name, so a suite pinned to
   * one tenant cannot run against another.
   */
  tenant?: string;
  /** The `aic` project directory; the `aic` provider only. */
  project?: string;
  /** Default: {@link configuredTenantProvider}, else the `aic` CLI. */
  provider?: TenantProvider;
  /**
   * `false` skips the registered and environment providers. Callers pass it
   * when they were handed an `io`: an injected `io` is a test seam for both
   * `aic` and HTTP, and a configured service account would mint its token
   * over the real network behind it.
   */
  useConfigured?: boolean;
}

/**
 * The provider a connection uses: the one passed, else the one registered
 * with `setTenantProvider` or configured by `RHINO_LOCAL_TENANT_URL`, else
 * this repo's `aic` agent.
 */
export function resolveTenantProvider(
  io: AicIo,
  options: ConnectOptions = {}
): TenantProvider {
  return (
    options.provider ??
    (options.useConfigured === false ? undefined : configuredTenantProvider()) ??
    aicCliProvider(io, {
      ...(options.tenant === undefined ? {} : { tenant: options.tenant }),
      ...(options.project === undefined ? {} : { project: options.project }),
    })
  );
}

export async function connectTenant(
  io: AicIo,
  options: ConnectOptions = {}
): Promise<TenantSession> {
  const provider = resolveTenantProvider(io, options);
  const description = await provider.describe();
  if (options.tenant !== undefined && options.tenant !== description.name) {
    throw new AicLaneError(
      `the tenant provider serves ${JSON.stringify(description.name)}, but this run asks for ${JSON.stringify(options.tenant)}`
    );
  }
  const token = await provider.getToken({ reason: "initial" });
  return {
    tenantName: description.name,
    baseUrl: stripSlash(description.baseUrl),
    token,
    provider,
  };
}

/**
 * This repo's provider: the tenant and bearer come from the `aic` agent
 * (`aic ctx list --json`, `aic whoami --token`), and logs from
 * `aic logs tx --wait`. The agent must be unlocked; every call passes
 * `--no-prompt` so a locked one fails instead of waiting for a password.
 */
export function aicCliProvider(
  io: AicIo,
  options: { tenant?: string; project?: string } = {}
): TenantProvider {
  const project = options.project ?? projectRoot();
  const noPrompt = ["--no-prompt", "--project", project];
  let chosen: Promise<TenantDescription> | undefined;
  function describe(): Promise<TenantDescription> {
    chosen ??= chooseContext(io, noPrompt, options.tenant);
    return chosen;
  }
  const logs: LogReader = {
    async transaction(id, readOptions = {}) {
      const { name } = await describe();
      const args = [...noPrompt, "logs", "tx", id, "--tenant", name, "--wait"];
      if (readOptions.waitMs !== undefined) {
        args.push("--timeout", String(Math.max(1, Math.ceil(readOptions.waitMs / 1000))));
      }
      const result = await io.aic(args);
      if (result.status !== 0) {
        throw new AicLaneError(formatLogsFailure(result), { status: result.status });
      }
      let parsed: unknown;
      try {
        parsed = JSON.parse(result.stdout);
      } catch (error) {
        throw new AicLaneError("aic logs tx did not print JSON", { cause: error });
      }
      if (!Array.isArray(parsed)) {
        throw new AicLaneError("aic logs tx did not print a JSON array");
      }
      return parsed as unknown[];
    },
  };
  return {
    describe,
    async getToken(request) {
      const { name } = await describe();
      const whoami = await io.aic([...noPrompt, "whoami", "--token", "--tenant", name]);
      const during = request.reason === "rejected" ? " while refreshing the bearer" : "";
      if (whoami.status !== 0) {
        throw new AicLaneError(
          `aic whoami --token failed${during} (status ${whoami.status}): ${trim(whoami.stderr || whoami.stdout)}. Unlock the agent (aic login) or pass a reachable tenant.`
        );
      }
      const token = whoami.stdout.trim();
      if (token.length === 0) {
        throw new AicLaneError(`aic whoami --token printed an empty token${during}`);
      }
      return token;
    },
    logs,
  };
}

async function chooseContext(
  io: AicIo,
  noPrompt: readonly string[],
  tenant: string | undefined
): Promise<TenantDescription> {
  const list = await io.aic([...noPrompt, "ctx", "list", "--json"]);
  if (list.status !== 0) {
    throw new AicLaneError(
      `aic ctx list failed (status ${list.status}): ${trim(list.stderr || list.stdout)}`
    );
  }
  const tenants = parseCtxList(list.stdout);
  const chosen =
    tenant !== undefined
      ? tenants.find((row) => row.name === tenant)
      : tenants.find((row) => row.current) ?? tenants[0];
  if (chosen === undefined) {
    throw new AicLaneError(
      tenant === undefined
        ? "aic ctx list returned no tenants"
        : `aic ctx list has no tenant named ${JSON.stringify(tenant)}`
    );
  }
  return { name: chosen.name, baseUrl: stripSlash(chosen.base_url) };
}

function formatLogsFailure(result: CliResult): string {
  const body = (result.stderr || result.stdout).trim();
  const hint = "Unlock the agent (`aic login`) if it is locked.";
  if (body.length === 0) {
    return `aic logs tx failed (status ${result.status}). ${hint}`;
  }
  if (/locked|aic login|session login/i.test(body)) {
    return body;
  }
  return `${body}\n${hint}`;
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
 * Ask the provider for a replacement bearer and adopt it.
 *
 * The history below is the `aic` provider's, and it is why a 401 must reach
 * the provider as `"rejected"`: a provider that caches has to know the token
 * it holds is dead, rather than handing it back.
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
export async function refreshSessionToken(session: TenantSession): Promise<void> {
  session.token = await session.provider.getToken({
    reason: "rejected",
    rejected: session.token,
  });
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
  await refreshSessionToken(session);
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
