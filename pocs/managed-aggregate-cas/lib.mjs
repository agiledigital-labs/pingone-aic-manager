// Shared helpers for the managed-aggregate CAS POC. Node 20+, no dependencies.
//
// Tenant: TENANT_BASE_URL, or the current context of the aic project at
// AIC_PROJECT (default: the repo root above this directory). The bearer comes
// from `aic whoami --token --host <base>`, which refuses to mint for a host
// that is not the context's own.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

const HERE = dirname(fileURLToPath(import.meta.url));
export const PROJECT = process.env.AIC_PROJECT || resolve(HERE, "../..");
const AIC = process.env.AIC || "aic";

export const PARENT = "alpha_poc_agg";
export const CHILD = "alpha_poc_child";
export const SINK = "alpha_poc_sink";
export const POC_TYPES = [PARENT, CHILD, SINK];

function aic(args) {
  return execFileSync(AIC, ["--no-prompt", ...args], {
    encoding: "utf8",
    env: { ...process.env, AIC_PROJECT: PROJECT },
  }).trim();
}

function baseUrl() {
  if (process.env.TENANT_BASE_URL) return process.env.TENANT_BASE_URL.replace(/\/$/, "");
  const ctx = readFileSync(join(PROJECT, ".aic/current-context"), "utf8").trim();
  // Sandbox only: these probes deliberately lose updates and write hooks.
  const allowed = process.env.POC_CONTEXT || "sandbox";
  if (ctx !== allowed) throw new Error(`current context is ${ctx}, not ${allowed}; refusing`);
  const toml = readFileSync(join(PROJECT, ".aic/config.toml"), "utf8");
  for (const block of toml.split(/^\[\[tenant\]\]/m).slice(1)) {
    const name = /^\s*name\s*=\s*"([^"]*)"/m.exec(block)?.[1];
    const url = /^\s*base_url\s*=\s*"([^"]*)"/m.exec(block)?.[1];
    if (name === ctx && url) return url.replace(/\/$/, "");
  }
  throw new Error(`no base_url for context ${ctx}`);
}

export const BASE = baseUrl();
let token = null;
let tokenAt = 0;

function bearer() {
  // Tokens live 898 s; re-mint well inside that.
  if (!token || Date.now() - tokenAt > 600_000) {
    token = aic(["whoami", "--token", "--host", BASE]);
    tokenAt = Date.now();
  }
  return token;
}

// One HTTP call. Never throws on a status; returns what a probe needs to
// record. `tx` becomes X-ForgeRock-TransactionId so logs can be fetched by id.
export async function call(method, path, { body, ifMatch, tx, timeoutMs = 120_000 } = {}) {
  const headers = {
    Authorization: `Bearer ${bearer()}`,
    "Accept-API-Version": "resource=1.0",
    "X-ForgeRock-TransactionId": tx || `poc-${randomUUID()}`,
  };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  if (ifMatch) headers["If-Match"] = ifMatch;
  const t0 = performance.now();
  let status = 0;
  let json = null;
  let error = null;
  try {
    const res = await fetch(BASE + path, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(timeoutMs),
    });
    status = res.status;
    const text = await res.text();
    try {
      json = text ? JSON.parse(text) : null;
    } catch {
      json = { raw: text.slice(0, 500) };
    }
  } catch (e) {
    error = String(e?.name || e);
  }
  const ms = performance.now() - t0;
  return { status, json, ms, tx: headers["X-ForgeRock-TransactionId"], error };
}

export async function ok(method, path, opts) {
  const r = await call(method, path, opts);
  if (r.status < 200 || r.status > 299) {
    throw new Error(`${method} ${path} -> ${r.status} ${r.error || JSON.stringify(r.json)?.slice(0, 300)}`);
  }
  return r.json;
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// --- records ---------------------------------------------------------------

export const rec = (type, id = "") => `/openidm/managed/${type}${id ? "/" + id : ""}`;

export async function createParent(fields) {
  return ok("POST", rec(PARENT) + "?_action=create", {
    body: { name: `poc-${Date.now()}`, inbox: [], entries: [], audit: [], auditSeq: 0, counter: 0, ...fields },
  });
}

export async function read(type, id, fields) {
  return ok("GET", rec(type, id) + (fields ? `?_fields=${fields}` : ""));
}

export async function queryAll(type, filter = "true", fields = "*") {
  const out = [];
  let cookie = null;
  do {
    const q = new URLSearchParams({ _queryFilter: filter, _fields: fields, _pageSize: "500" });
    if (cookie) q.set("_pagedResultsCookie", cookie);
    const r = await ok("GET", `${rec(type)}?${q}`);
    out.push(...r.result);
    cookie = r.pagedResultsCookie;
  } while (cookie);
  return out;
}

export async function sinksFor(parentId) {
  return queryAll(SINK, `parent eq "${parentId}"`);
}

export async function deleteAll(type) {
  const ids = (await queryAll(type, "true", "_id")).map((r) => r._id);
  for (let i = 0; i < ids.length; i += 20) {
    await Promise.all(ids.slice(i, i + 20).map((id) => call("DELETE", rec(type, id))));
  }
  return ids.length;
}

// --- concurrency ------------------------------------------------------------

// Fire `n` calls at once (each from makeCall(i)) and return their results.
export async function burst(n, makeCall) {
  return Promise.all(Array.from({ length: n }, (_, i) => makeCall(i)));
}

export function summarise(results) {
  const byStatus = {};
  for (const r of results) {
    const k = r.error ? `err:${r.error}` : String(r.status);
    byStatus[k] = (byStatus[k] || 0) + 1;
  }
  const ms = results.map((r) => r.ms).sort((a, b) => a - b);
  const pct = (p) => Math.round(ms[Math.min(ms.length - 1, Math.floor((p / 100) * ms.length))]);
  return { n: results.length, byStatus, p50: pct(50), p90: pct(90), max: Math.round(ms[ms.length - 1]) };
}
