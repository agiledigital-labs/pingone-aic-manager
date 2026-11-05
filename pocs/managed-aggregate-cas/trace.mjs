// Summarise the writes one IDM change caused, from its transaction id.
//
//   node trace.mjs <txid-or-stem>
//
// Fetches idm-access and idm-activity with `aic logs tx --wait --output` and
// prints, per stored transaction id, the write count by operation and object
// type, the object ids, and the access event's status and latency.
//
// Reads are absent from both sources. A hook that wants them counted has to
// log them itself, with context.transactionId.transactionId.value in the
// message. Script logger lines are not returned by `aic logs tx`.
//
// Matching is a prefix match. Number any ids that share a stem so that none
// is a prefix of another (`<stem>-01`, not `<stem>-1`).

import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const AIC = process.env.AIC || "aic";

function objectParts(objectId) {
  const parts = String(objectId || "").split("/");
  if (parts[0] === "managed" && parts.length >= 3) {
    return { type: parts[1], id: parts.slice(2).join("/") };
  }
  return { type: parts.filter(Boolean).join("/") || "?", id: "" };
}

// Events as `aic logs tx --output` writes them. Drops every field a committed
// note must not carry (server, headers, user id, before/after documents).
export function summariseEvents(events) {
  const byTx = new Map();
  for (const ev of events) {
    const payload = ev?.payload;
    if (!payload || typeof payload !== "object") continue;
    const tx = String(payload.transactionId || "(none)");
    if (!byTx.has(tx)) byTx.set(tx, { access: [], writes: [] });
    const row = byTx.get(tx);
    if (ev.source === "idm-access" || payload.eventName === "access") {
      const response = payload.response || {};
      row.access.push({
        method: payload.http?.request?.method || null,
        status: response.status || null,
        statusCode: response.statusCode || null,
        elapsedMs: response.elapsedTime ?? null,
        units: response.elapsedTimeUnits || null,
      });
      continue;
    }
    if (ev.source === "idm-activity" || payload.eventName === "activity") {
      const obj = objectParts(payload.objectId);
      row.writes.push({
        operation: payload.operation || "?",
        type: obj.type,
        id: obj.id,
        status: payload.status || null,
      });
    }
  }
  return [...byTx.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([transactionId, group]) => ({ transactionId, ...group }));
}

export function fetchEvents(id, { wait = true, timeout = 75 } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "poc-trace-"));
  const out = join(dir, "logs.json");
  const args = [
    "--no-prompt",
    "logs",
    "tx",
    id,
    "--source",
    "idm-access,idm-activity",
    "--output",
    out,
  ];
  if (wait) args.push("--wait", "--timeout", String(timeout));
  try {
    execFileSync(AIC, args, { stdio: ["ignore", "ignore", "inherit"] });
    return JSON.parse(readFileSync(out, "utf8"));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

export function fetchSummary(id, opts) {
  return summariseEvents(fetchEvents(id, opts));
}

// idm-core has no transaction id on script lines. A short window grep for
// ERROR is the error-line check; it is not proof the window was empty when
// `exit` is non-zero (the scan stopped on --max-time).
export function fetchCoreErrors(begin, end) {
  const dir = mkdtempSync(join(tmpdir(), "poc-core-"));
  const out = join(dir, "core.json");
  const args = [
    "--no-prompt",
    "logs",
    "grep",
    "ERROR",
    "--source",
    "idm-core",
    "--begin",
    begin,
    "--end",
    end,
    "--limit",
    "15",
    "--max-time",
    "90s",
    "--output",
    out,
  ];
  const r = spawnSync(AIC, args, {
    encoding: "utf8",
    env: process.env,
    maxBuffer: 4 * 1024 * 1024,
  });
  let events = [];
  try {
    const parsed = JSON.parse(readFileSync(out, "utf8"));
    if (Array.isArray(parsed)) events = parsed;
  } catch {
    events = [];
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  return { exit: r.status, stderr: r.stderr || "", events };
}

export function formatSummary(id, rows) {
  const lines = [
    `query: ${id}`,
    "reads are not in idm-access or idm-activity; a hook counts them by logging",
    "context.transactionId.transactionId.value with the read.",
    `transactions: ${rows.length}`,
  ];
  for (const row of rows) {
    lines.push("");
    lines.push(row.transactionId);
    if (!row.access.length) lines.push("  access: (none)");
    for (const access of row.access) {
      const elapsed = access.elapsedMs == null ? "?" : `${access.elapsedMs}${access.units === "MILLISECONDS" ? "ms" : access.units || ""}`;
      lines.push(`  access: ${access.method || "?"} ${access.status || "?"} ${access.statusCode || "?"} ${elapsed}`);
    }
    const groups = new Map();
    for (const write of row.writes) {
      const key = `${write.operation} ${write.type}`;
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(`${write.id}${write.status && write.status !== "SUCCESS" ? ` [${write.status}]` : ""}`);
    }
    if (!groups.size) lines.push("  writes: (none)");
    for (const [key, ids] of groups) {
      lines.push(`  ${key} × ${ids.length}: ${ids.join(" ")}`);
    }
  }
  return lines.join("\n");
}

if (process.argv[1] && process.argv[1].endsWith("trace.mjs")) {
  const id = process.argv[2];
  if (!id) {
    console.error("usage: node trace.mjs <txid-or-stem>");
    process.exit(2);
  }
  const rows = fetchSummary(id);
  console.log(formatSummary(id, rows));
}
