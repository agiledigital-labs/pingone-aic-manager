// The POC's experiments. `node experiments.mjs E1 [E2 ...]` runs them in order
// and writes each result to .state/results/<id>-<time>.json; the console gets
// a summary. Requires `node setup.mjs install` first.

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { call, ok, read, rec, createParent, sinksFor, burst, summarise, sleep, queryAll, PARENT, CHILD, SINK } from "./lib.mjs";
import { fetchSummary } from "./trace.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, ".state/results");
mkdirSync(OUT, { recursive: true });

const REPEATS = Number(process.env.REPEATS || 3);
const patch = (id, ops, opts = {}) => call("PATCH", rec(PARENT, id), { body: ops, ...opts });
const addInbox = (id, entry, opts) => patch(id, [{ operation: "add", field: "/inbox/-", value: entry }], opts);

// What `derived` should be, recomputed from the stored entries.
function recompute(p) {
  let sum = 0;
  let best = null;
  for (const e of p.entries) {
    const v = Number(e.value) || 0;
    sum += v;
    if (best === null || v > best.value) best = { id: e.id, value: v };
  }
  const count = p.entries.length;
  const outcome = p.threshold > 0 && count >= p.threshold ? "closed" : "open";
  return { count, sum, best: best?.id ?? null, outcome };
}

function consistent(p) {
  const want = recompute(p);
  const d = p.derived || {};
  return ["count", "sum", "best", "outcome"].every((k) => d[k] === want[k]);
}

// Sinks grouped: onUpdate runs, postUpdate runs, and runs per request (the
// trace sink records the request's patch operations, so equal `ops` means
// the same request ran onUpdate more than once — a retry).
function sinkStats(sinks) {
  const on = sinks.filter((s) => s.kind === "onUpdate");
  const post = sinks.filter((s) => s.kind === "postUpdate");
  const perRequest = {};
  for (const s of on) perRequest[s.ops] = (perRequest[s.ops] || 0) + 1;
  const runs = Object.values(perRequest);
  // On a retry, a later run of the same request must see a newer base.
  const staleRetry = Object.keys(perRequest).filter((ops) => {
    const rs = on.filter((s) => s.ops === ops).sort((a, b) => a.at - b.at);
    return rs.some((r, i) => i > 0 && r.oldRev === rs[i - 1].oldRev);
  }).length;
  return {
    onUpdateRuns: on.length,
    postUpdateRuns: post.length,
    distinctPostRevs: new Set(post.map((s) => s.rev)).size,
    requestsRetried: runs.filter((n) => n > 1).length,
    maxRunsPerRequest: runs.length ? Math.max(...runs) : 0,
    retriesWithSameBase: staleRetry,
  };
}

function save(id, data) {
  const file = join(OUT, `${id}-${new Date().toISOString().replace(/[:.]/g, "")}.json`);
  writeFileSync(file, JSON.stringify(data, null, 2));
  return file;
}

// --- experiments -------------------------------------------------------------

// E1 + E6 + E7 + E17: N concurrent revision-less appends.
async function concurrentAppend(n, { trace = true, via = "rest" } = {}) {
  const p = await createParent({ mode: "fold", trace, postMode: "sink" });
  const send = via === "endpoint" ? endpointAppend : addInbox;
  const results = await burst(n, (i) => send(p._id, { id: `e${i}`, value: (i * 7919) % 101 }));
  const after = await read(PARENT, p._id);
  const ids = new Set(after.entries.map((e) => e.id));
  const sinks = await sinksFor(p._id);
  return {
    n,
    via,
    trace,
    http: summarise(results),
    entries: after.entries.length,
    allPresent: Array.from({ length: n }, (_, i) => `e${i}`).every((x) => ids.has(x)),
    duplicates: after.entries.length - ids.size,
    inboxLeft: after.inbox.length,
    derivedConsistent: consistent(after),
    derivedRuns: after.derived?.runs,
    ...sinkStats(sinks),
  };
}

async function E1() {
  const runs = [];
  for (const n of [2, 10, 50]) for (let r = 0; r < REPEATS; r++) runs.push(await concurrentAppend(n));
  return runs;
}

async function E2() {
  const p = await createParent({ mode: "fold" });
  const rev = (await read(PARENT, p._id))._rev;
  const first = await addInbox(p._id, { id: "a", value: 1 }, { ifMatch: rev });
  const second = await addInbox(p._id, { id: "b", value: 2 }, { ifMatch: rev });
  const after = await read(PARENT, p._id);
  return {
    first: first.status,
    second: second.status,
    secondBody: second.json,
    entries: after.entries.map((e) => e.id),
  };
}

// E3: N concurrent full replaces, each built from its own prior read.
async function E3() {
  const runs = [];
  for (const n of [5, 20]) {
    for (let r = 0; r < REPEATS; r++) {
      const p = await createParent({ mode: "fold" });
      const results = await burst(n, async (i) => {
        const cur = await read(PARENT, p._id);
        const { _id, _rev, ...body } = cur;
        body.entries = [...cur.entries, { id: `e${i}`, value: i }];
        return call("PUT", rec(PARENT, p._id), { body });
      });
      const after = await read(PARENT, p._id);
      runs.push({ n, http: summarise(results), entries: after.entries.length, lost: n - after.entries.length, derivedConsistent: consistent(after) });
    }
  }
  return runs;
}

// E4: client-computed array replace through PATCH (read outside the CAS).
async function E4() {
  const runs = [];
  for (const n of [5, 20]) {
    for (let r = 0; r < REPEATS; r++) {
      const p = await createParent({ mode: "fold" });
      const results = await burst(n, async (i) => {
        const cur = await read(PARENT, p._id);
        return patch(p._id, [{ operation: "replace", field: "/entries", value: [...cur.entries, { id: `e${i}`, value: i }] }]);
      });
      const after = await read(PARENT, p._id);
      runs.push({ n, http: summarise(results), entries: after.entries.length, lost: n - after.entries.length, derivedConsistent: consistent(after) });
    }
  }
  return runs;
}

// E5: server-side operations: increment, and insert at a fixed index.
async function E5() {
  const runs = [];
  for (const n of [10, 50]) {
    const p = await createParent({ mode: "plain" });
    const inc = await burst(n, () => patch(p._id, [{ operation: "increment", field: "/counter", value: 1 }]));
    const mid = await read(PARENT, p._id);
    const ins = await burst(n, (i) => patch(p._id, [{ operation: "add", field: "/items/0", value: { w: i } }]));
    const after = await read(PARENT, p._id);
    runs.push({
      n,
      increment: { http: summarise(inc), counter: mid.counter, sample: inc.find((r) => r.status !== 200)?.json },
      insertAt0: { http: summarise(ins), items: (after.items || []).length, distinct: new Set((after.items || []).map((x) => x.w)).size },
    });
  }
  return runs;
}

// E8: postUpdate throws after the write.
async function E8() {
  const p = await createParent({ mode: "fold", postMode: "throw" });
  const r = await addInbox(p._id, { id: "a", value: 1 });
  const after = await read(PARENT, p._id);
  return { status: r.status, body: r.json, entriesAfter: after.entries.map((e) => e.id), derived: after.derived };
}

// E9: a throw in onUpdate as an invariant guard, sequential and concurrent.
async function E9() {
  const p = await createParent({ mode: "fold", threshold: 3 });
  for (const id of ["a", "b", "c"]) await addInbox(p._id, { id, value: 1 });
  const refused = await addInbox(p._id, { id: "d", value: 1 });
  const afterSeq = await read(PARENT, p._id);
  const concurrent = [];
  for (let r = 0; r < REPEATS; r++) {
    const q = await createParent({ mode: "fold", threshold: 5 });
    const res = await burst(20, (i) => addInbox(q._id, { id: `e${i}`, value: i }));
    const after = await read(PARENT, q._id);
    concurrent.push({ threshold: 5, n: 20, http: summarise(res), entries: after.entries.length, outcome: after.derived?.outcome, derivedConsistent: consistent(after) });
  }
  return {
    sequential: { status: refused.status, body: refused.json, entries: afterSeq.entries.map((e) => e.id), outcome: afterSeq.derived?.outcome },
    concurrent,
  };
}

// E10: concurrent link writes while onUpdate derives from the relationship.
async function E10() {
  const runs = [];
  for (const rel of ["children", "gchildren"]) {
    for (const n of [5, 20]) {
      for (let r = 0; r < REPEATS; r++) {
        const p = await createParent({ mode: "children", rel, trace: true });
        const kids = await burst(n, (i) => ok("POST", rec(CHILD) + "?_action=create", { body: { label: `k${i}`, value: i + 1 } }));
        const res = await burst(n, (i) =>
          patch(p._id, [{ operation: "add", field: `/${rel}/-`, value: { _ref: `managed/${CHILD}/${kids[i]._id}`, _refProperties: { status: "active" } } }]),
        );
        await sleep(1000);
        const after = await read(PARENT, p._id);
        const links = await ok("GET", `${rec(PARENT, p._id)}/${rel}?_queryFilter=true&_fields=_ref`);
        const sinks = await sinksFor(p._id);
        runs.push({
          rel,
          n,
          http: summarise(res),
          links: links.result.length,
          derivedCount: after.derived?.count,
          derivedSum: after.derived?.sum,
          wantSum: (n * (n + 1)) / 2,
          ...sinkStats(sinks),
        });
      }
    }
  }
  return runs;
}

// E11: an edge _refProperties write — does the parent's onUpdate run, and does
// it go through the parent's revision?
async function E11() {
  const out = [];
  for (const rel of ["children", "gchildren"]) {
    const p = await createParent({ mode: "children", rel, trace: true });
    const kid = await ok("POST", rec(CHILD) + "?_action=create", { body: { label: "k", value: 5 } });
    await patch(p._id, [{ operation: "add", field: `/${rel}/-`, value: { _ref: `managed/${CHILD}/${kid._id}`, _refProperties: { status: "active" } } }]);
    const before = await read(PARENT, p._id);
    const sinksBefore = (await sinksFor(p._id)).length;
    const edges = await ok("GET", `${rec(PARENT, p._id)}/${rel}?_queryFilter=true&_fields=_ref,_refProperties`);
    const edgeId = edges.result[0]._id;
    const viaEdge = await call("PATCH", `${rec(PARENT, p._id)}/${rel}/${edgeId}`, {
      body: [{ operation: "replace", field: "/_refProperties/status", value: "inactive" }],
    });
    await sleep(1000);
    const after = await read(PARENT, p._id);
    const sinksAfter = (await sinksFor(p._id)).length;
    out.push({
      rel,
      edgePatch: viaEdge.status,
      edgeBody: viaEdge.status === 200 ? undefined : viaEdge.json,
      parentRevMoved: before._rev !== after._rev,
      onUpdateRanForEdge: sinksAfter > sinksBefore,
      derivedActiveBefore: before.derived?.active,
      derivedActiveAfter: after.derived?.active,
      edgeStatusNow: (await ok("GET", `${rec(PARENT, p._id)}/${rel}/${edgeId}`)).
        _refProperties?.status,
    });
  }
  return out;
}

// E12: a child changes after the parent derived from it.
async function E12() {
  const out = [];
  for (const rel of ["children", "gchildren"]) {
    const p = await createParent({ mode: "children", rel, trace: true });
    const kid = await ok("POST", rec(CHILD) + "?_action=create", { body: { label: "k", value: 5 } });
    await patch(p._id, [{ operation: "add", field: `/${rel}/-`, value: { _ref: `managed/${CHILD}/${kid._id}`, _refProperties: { status: "active" } } }]);
    await patch(p._id, [{ operation: "replace", field: "/counter", value: 1 }]);
    const before = await read(PARENT, p._id);
    const sinksBefore = (await sinksFor(p._id)).length;
    const childPatch = await call("PATCH", rec(CHILD, kid._id), { body: [{ operation: "replace", field: "/value", value: 50 }] });
    await sleep(2000);
    const after = await read(PARENT, p._id);
    out.push({
      rel,
      childPatch: childPatch.status,
      derivedSumBefore: before.derived?.sum,
      derivedSumAfter: after.derived?.sum,
      parentRevMoved: before._rev !== after._rev,
      onUpdateRan: (await sinksFor(p._id)).length > sinksBefore,
    });
  }
  return out;
}

// E13: the same appends through a custom endpoint's openidm.patch.
const ENDPOINT = "pocagg";
function endpointAppend(id, entry) {
  return call("POST", `/openidm/endpoint/${ENDPOINT}?_action=append`, { body: { id, entry } });
}
async function E13() {
  const src = [
    "(function () {",
    "  var c = request.content;",
    `  var r = openidm.patch("managed/${PARENT}/" + c.id, null, [{ operation: "add", field: "/inbox/-", value: c.entry }]);`,
    "  return { _rev: r._rev, count: r.derived ? r.derived.count : null };",
    "})();",
  ].join("\n");
  const put = await call("PUT", `/openidm/config/endpoint/${ENDPOINT}`, { body: { type: "text/javascript", source: src, context: `endpoint/${ENDPOINT}` } });
  if (put.status !== 200 && put.status !== 201) throw new Error(`endpoint PUT ${put.status} ${JSON.stringify(put.json)}`);
  // Wait for the endpoint to answer.
  const probe = await createParent({ mode: "fold" });
  for (let i = 0; i < 30; i++) {
    const r = await endpointAppend(probe._id, { id: "probe", value: 0 });
    if (r.status === 200) break;
    await sleep(2000);
  }
  const runs = [];
  for (const n of [10, 50]) for (let r = 0; r < REPEATS; r++) runs.push(await concurrentAppend(n, { via: "endpoint" }));
  return { endpointPut: put.status, runs };
}

// E14: heavy contention on one parent; stops climbing at the first failure.
async function E14() {
  const runs = [];
  for (const n of (process.env.E14_N || "100,200").split(",").map(Number)) {
    const run = await concurrentAppend(n, { trace: process.env.E14_TRACE !== "0" });
    runs.push(run);
    console.log(`  E14 n=${n}`, JSON.stringify(run.http));
    if (Object.keys(run.http.byStatus).some((k) => k !== "200")) break;
  }
  return runs;
}

// E15: the parent's own hooks patch the parent.
async function E15() {
  const p = await createParent({ mode: "selfpatch", trace: true });
  const on = await call("PATCH", rec(PARENT, p._id), { body: [{ operation: "add", field: "/inbox/-", value: { id: "a", value: 1, selfpatch: true } }], timeoutMs: 90_000 });
  const afterOn = await read(PARENT, p._id);
  const q = await createParent({ mode: "fold", postMode: "selfpatch", trace: true });
  const post = await call("PATCH", rec(PARENT, q._id), { body: [{ operation: "add", field: "/inbox/-", value: { id: "a", value: 1 } }], timeoutMs: 90_000 });
  const afterPost = await read(PARENT, q._id);
  return {
    fromOnUpdate: { status: on.status, ms: Math.round(on.ms), body: on.status === 200 ? { counter: on.json.counter, entries: on.json.entries.length } : on.json, stored: { counter: afterOn.counter, entries: afterOn.entries.length, rev: afterOn._rev }, onUpdateRuns: (await sinksFor(p._id)).filter((s) => s.kind === "onUpdate").length },
    fromPostUpdate: { status: post.status, ms: Math.round(post.ms), responseCounter: post.json?.counter, stored: { counter: afterPost.counter, entries: afterPost.entries.length }, onUpdateRuns: (await sinksFor(q._id)).filter((s) => s.kind === "onUpdate").length },
  };
}

// E16: a patch that changes nothing.
async function E16() {
  const p = await createParent({ mode: "fold", trace: true, name: "same" });
  const r0 = await read(PARENT, p._id);
  const s0 = (await sinksFor(p._id)).length;
  const same = await patch(p._id, [{ operation: "replace", field: "/name", value: "same" }]);
  const r1 = await read(PARENT, p._id);
  const s1 = (await sinksFor(p._id)).length;
  // The hook itself makes every run a change (derived.runs), so also try a
  // type whose hook changes nothing: mode "plain".
  const q = await createParent({ mode: "plain", trace: true, name: "same" });
  const q0 = await read(PARENT, q._id);
  const qs0 = (await sinksFor(q._id)).length;
  const sameQ = await patch(q._id, [{ operation: "replace", field: "/name", value: "same" }]);
  const q1 = await read(PARENT, q._id);
  const qs1 = (await sinksFor(q._id)).length;
  return {
    foldMode: { status: same.status, revMoved: r0._rev !== r1._rev, onUpdateRan: s1 > s0 },
    plainMode: { status: sameQ.status, revMoved: q0._rev !== q1._rev, onUpdateRan: qs1 > qs0 },
  };
}

// E18: cost of a growing array on the parent.
async function E18() {
  const out = [];
  for (const size of (process.env.E18_SIZES || "10,100,1000,5000").split(",").map(Number)) {
    const entries = Array.from({ length: size }, (_, i) => ({ id: `s${i}`, value: i % 100, note: "x".repeat(40) }));
    const p = await call("POST", rec(PARENT) + "?_action=create", { body: { name: `size-${size}`, mode: "fold", inbox: [], entries, audit: [] } });
    if (p.status !== 201 && p.status !== 200) {
      out.push({ size, create: p.status, body: p.json });
      break;
    }
    const seq = [];
    for (let i = 0; i < 5; i++) seq.push(await addInbox(p.json._id, { id: `n${i}`, value: 1 }));
    const conc = await burst(10, (i) => addInbox(p.json._id, { id: `c${i}`, value: 1 }));
    const after = await read(PARENT, p.json._id);
    out.push({
      size,
      bytes: JSON.stringify(entries).length,
      createMs: Math.round(p.ms),
      sequential: summarise(seq),
      concurrent10: summarise(conc),
      entries: after.entries.length,
      wantEntries: size + 15,
      derivedConsistent: consistent(after),
    });
  }
  return out;
}

// E19: the audit trail written by onUpdate in memory.
function chainOk(audit) {
  const last = {};
  let bad = 0;
  for (const e of [...audit].sort((a, b) => a.seq - b.seq)) {
    if (e.path in last && JSON.stringify(last[e.path]) !== JSON.stringify(e.from)) bad++;
    last[e.path] = e.to;
  }
  return bad;
}

async function E19() {
  const cost = [];
  for (const k of [1, 10, 50]) {
    const ops = (i) => [{ operation: "replace", field: `/data/sub/f${i}`, value: `v${i}-${Date.now()}` }];
    // (a) today's shape: one data PATCH and one audit PATCH per change.
    const a = await createParent({ mode: "plain", data: { sub: {} } });
    const t0 = performance.now();
    for (let i = 0; i < k; i++) {
      await patch(a._id, ops(i));
      await patch(a._id, [{ operation: "add", field: "/audit/-", value: { path: `/data/sub/f${i}`, at: Date.now() } }]);
    }
    const ta = performance.now() - t0;
    // (b) one PATCH per change; onUpdate writes the audit.
    const b = await createParent({ mode: "audit", data: { sub: {} } });
    const t1 = performance.now();
    for (let i = 0; i < k; i++) await patch(b._id, ops(i));
    const tb = performance.now() - t1;
    // (c) one PATCH carrying every change; onUpdate writes the audit.
    const c = await createParent({ mode: "audit", data: { sub: {} } });
    const t2 = performance.now();
    const rc = await patch(c._id, Array.from({ length: k }, (_, i) => ops(i)[0]));
    const tc = performance.now() - t2;
    const [ra, rb, rc2] = await Promise.all([read(PARENT, a._id), read(PARENT, b._id), read(PARENT, c._id)]);
    cost.push({
      k,
      a_twoPatchesPerChange: { calls: 2 * k, ms: Math.round(ta), audit: ra.audit.length },
      b_onePatchPerChange: { calls: k, ms: Math.round(tb), audit: rb.audit.length, chainBreaks: chainOk(rb.audit) },
      c_onePatchTotal: { calls: 1, ms: Math.round(tc), status: rc.status, audit: rc2.audit.length, chainBreaks: chainOk(rc2.audit), opsSeen: rc2.audit[0]?.ops },
    });
  }

  // Contention: N writers each change a shared leaf, a nested leaf of their
  // own, and append an item. Every committed write must produce exactly
  // three audit entries, the shared leaf's chain must be unbroken, and no run
  // id may appear in the trail that did not commit.
  const contention = [];
  for (const n of [10, 50]) {
    for (let r = 0; r < REPEATS; r++) {
      const p = await createParent({ mode: "audit", trace: true, data: { shared: "start", per: {} }, items: [] });
      const res = await burst(n, (i) =>
        patch(p._id, [
          { operation: "replace", field: "/data/shared", value: `w${i}` },
          { operation: "replace", field: `/data/per/w${i}`, value: i },
          { operation: "add", field: "/items/-", value: { w: i, status: "new" } },
        ]),
      );
      const after = await read(PARENT, p._id);
      const committed = res.filter((x) => x.status === 200).length;
      const runIds = new Set(after.audit.map((e) => e.runId));
      const seqs = after.audit.map((e) => e.seq);
      const sinks = await sinksFor(p._id);
      contention.push({
        n,
        http: summarise(res),
        committed,
        auditEntries: after.audit.length,
        wantAudit: committed * 3,
        runIdsInTrail: runIds.size,
        sharedChainBreaks: chainOk(after.audit.filter((e) => e.path === "/data/shared")),
        seqDistinct: new Set(seqs).size === seqs.length,
        seqContiguous: Math.max(...seqs) === seqs.length,
        finalShared: after.data.shared,
        lastSharedTo: after.audit.filter((e) => e.path === "/data/shared").sort((x, y) => x.seq - y.seq).pop()?.to,
        ...sinkStats(sinks),
      });
    }
  }
  return { cost, contention };
}

// E20 under contention. N revision-less appends, each with its own transaction
// id under one stem. Ids are zero-padded so none is a prefix of another
// (`docs/api/08-logs.md`). The stem query is supposed to return every request;
// one per-id query must return only that request.
function entryFromOps(ops) {
  const text = typeof ops === "string" ? ops : JSON.stringify(ops ?? "");
  return /"id"\s*:\s*"([^"]+)"/.exec(text)?.[1] || null;
}

function belongs(stored, root) {
  return stored === root || stored.startsWith(`${root}/`);
}

async function E20() {
  const n = Number(process.env.E20_N || 10);
  const stem = `p20${randomUUID().replace(/-/g, "")}`;
  const txs = Array.from({ length: n }, (_, i) => `${stem}-${String(i).padStart(2, "0")}`);
  for (const a of txs) {
    for (const b of txs) {
      if (a !== b && b.startsWith(a)) throw new Error(`${a} is a prefix of ${b}`);
    }
  }
  const p = await createParent({ mode: "fold", trace: true, postMode: "sink" });
  const results = await burst(n, (i) => addInbox(p._id, { id: `e${i}`, value: i }, { tx: txs[i] }));
  const after = await read(PARENT, p._id);
  const sinks = await sinksFor(p._id);
  const onByEntry = {};
  for (const sink of sinks) {
    if (sink.kind !== "onUpdate") continue;
    const entry = entryFromOps(sink.ops);
    onByEntry[entry] = (onByEntry[entry] || 0) + 1;
  }
  const post = sinks.filter((s) => s.kind === "postUpdate").length;
  let rows = await fetchSummary(stem);
  if (rows.length < n) rows = await fetchSummary(stem, { timeout: 90 });
  const one = await fetchSummary(txs[0], { wait: false });
  const perRequest = txs.map((tx, i) => {
    const mine = rows.filter((row) => belongs(row.transactionId, tx));
    const writes = mine.flatMap((row) => row.writes);
    const parentPatches = writes.filter((w) => w.type === PARENT && w.operation === "PATCH");
    const sinkCreates = writes.filter((w) => w.type === SINK && w.operation === "CREATE");
    const other = writes.filter((w) => !(w.type === PARENT && w.operation === "PATCH") && !(w.type === SINK && w.operation === "CREATE"));
    return {
      entry: `e${i}`,
      http: results[i].status,
      storedIds: mine.map((row) => row.transactionId),
      access: mine.flatMap((row) => row.access).map((a) => ({ method: a.method, status: a.status, statusCode: a.statusCode, elapsedMs: a.elapsedMs })),
      parentPatches: parentPatches.length,
      parentIds: [...new Set(parentPatches.map((w) => w.id))],
      parentStatuses: [...new Set(parentPatches.map((w) => w.status))],
      sinkCreates: sinkCreates.length,
      onUpdateSinks: onByEntry[`e${i}`] || 0,
      otherWrites: other.length,
    };
  });
  const stray = rows.filter((row) => !txs.some((tx) => belongs(row.transactionId, tx))).map((row) => row.transactionId);
  return {
    n,
    http: summarise(results),
    entries: after.entries.length,
    allPresent: Array.from({ length: n }, (_, i) => `e${i}`).every((id) => after.entries.some((e) => e.id === id)),
    derivedConsistent: consistent(after),
    sinkRecords: sinks.length,
    onUpdateSinks: sinks.filter((s) => s.kind === "onUpdate").length,
    postUpdateSinks: post,
    logTransactions: rows.length,
    logSinkCreates: perRequest.reduce((sum, row) => sum + row.sinkCreates, 0),
    logParentPatches: perRequest.reduce((sum, row) => sum + row.parentPatches, 0),
    oneIdReturnsOnlyItself: one.length > 0 && one.every((row) => belongs(row.transactionId, txs[0])),
    oneIdTransactionCount: one.length,
    strayStoredIds: stray.length,
    perRequest,
  };
}

const ALL = { E1, E2, E3, E4, E5, E8, E9, E10, E11, E12, E13, E14, E15, E16, E18, E19, E20 };
for (const id of process.argv.slice(2)) {
  if (!ALL[id]) {
    console.error(`unknown experiment ${id}; have ${Object.keys(ALL).join(" ")}`);
    process.exit(2);
  }
  const t0 = Date.now();
  const result = await ALL[id]();
  const file = save(id, result);
  console.log(`== ${id} (${Math.round((Date.now() - t0) / 1000)}s) -> ${file}`);
  console.log(JSON.stringify(result, null, 1).slice(0, 6000));
}
export { queryAll, SINK };
