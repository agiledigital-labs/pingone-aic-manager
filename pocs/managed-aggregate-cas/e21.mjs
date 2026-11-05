// E21 — relationship notification, both ends.
//
//   node e21.mjs repo|table|deletewait|placement|fanout|shape|peers|owner|attrs|trace|cas|all
//
// Requires `node setup.mjs install` first. Each part configures the POC
// types, proves a direct PATCH fires the hooks it counts, then measures.
// Raw JSON goes to .state/results/ (gitignored). A direct-PATCH control that
// does not fire aborts that part.

import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import {
  call, read, rec, createParent, sinksFor, sinksForChild, sleep, burst, summarise,
  PARENT, CHILD,
} from "./lib.mjs";
import { fetchSummary } from "./trace.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, ".state/results");
mkdirSync(OUT, { recursive: true });

const WAIT_MS = Number(process.env.E21_WAIT || 6000);
const LATE_MS = Number(process.env.E21_LATE || 30000);

const MAIN = [
  { name: "mapped", parentRel: "children", childRel: "parent", childMany: false },
  { name: "generic", parentRel: "gchildren", childRel: "gparent", childMany: false },
];
const PEERS = { name: "peers", parentRel: "peers", childRel: "peerOf", childMany: true };
const OWNER = { name: "owner", parentRel: "owned", childRel: "owner", childMany: false };

const TABLE_WRITES = [
  "create-reverse", "replace-reverse", "child-plain", "child-delete",
  "edge-parent", "edge-child", "parent-add", "parent-plain", "parent-delete",
];

function scrub(s) {
  return String(s ?? "")
    .replace(/https?:\/\/\S+/g, "<url>")
    .replace(/[a-z0-9-]+\.forgeblocks\.com/gi, "<tenant>")
    .replace(/[a-z0-9-]+\.id\.forgerock\.io/gi, "<tenant>")
    .slice(0, 400);
}

function save(part, data) {
  const file = join(OUT, `E21-${part}.json`);
  writeFileSync(file, JSON.stringify(data, null, 2));
  console.log(`saved ${file}`);
  return file;
}

function configure(env) {
  const r = spawnSync(process.execPath, ["setup.mjs", "configure"], {
    cwd: HERE,
    env: {
      ...process.env,
      NOTIFY: "",
      NOTIFY_SPEC: "",
      FLAG_AT: "node",
      FANOUT: "off",
      RETURN_BY_DEFAULT: "0",
      ...env,
    },
    encoding: "utf8",
  });
  const output = scrub(`${r.stdout || ""}\n${r.stderr || ""}`).slice(-500);
  if (r.status !== 0) return { ok: false, output };
  console.log(output.split("\n").filter((l) => l.includes("confirmed") || l.includes("PUT")).join("\n") || "configured");
  return { ok: true, output };
}

function runSetup(cmd) {
  const r = spawnSync(process.execPath, ["setup.mjs", cmd], {
    cwd: HERE,
    env: process.env,
    encoding: "utf8",
  });
  return { ok: r.status === 0, output: scrub(`${r.stdout || ""}\n${r.stderr || ""}`).slice(-500) };
}

// Flags and notifyRelationships actually stored, per POC relationship and per
// plain field that names relationships. No scripts, no ids.
async function liveShape() {
  const r = await call("GET", "/openidm/config/managed");
  if (r.status !== 200) return { status: r.status };
  const out = {};
  for (const o of r.json.objects || []) {
    if (o.name !== PARENT && o.name !== CHILD) continue;
    const props = o.schema?.properties || {};
    const fields = {};
    for (const [k, prop] of Object.entries(props)) {
      if (!prop || typeof prop !== "object") continue;
      const node = prop.type === "array" ? prop.items : prop;
      const rel = prop.type === "relationship" || node?.type === "relationship";
      const named = prop.notifyRelationships || node?.notifyRelationships || null;
      if (!rel && !named) continue;
      fields[k] = {
        notify: node?.notify ?? null,
        notifySelf: node?.notifySelf ?? null,
        rcNotify: node?.resourceCollection?.[0]?.notify ?? null,
        rcNotifySelf: node?.resourceCollection?.[0]?.notifySelf ?? null,
        returnByDefault: prop.returnByDefault ?? null,
        notifyRelationships: named,
      };
    }
    out[o.name] = {
      object: o.notifyRelationships ?? null,
      schema: o.schema?.notifyRelationships ?? null,
      fields,
    };
  }
  return out;
}

function countKinds(rows) {
  const n = (kind) => rows.filter((s) => s.kind === kind).length;
  return {
    parentOn: n("onUpdate"),
    parentPost: n("postUpdate"),
    childOn: n("child-onUpdate"),
    childPost: n("child-postUpdate"),
    childDel: n("child-onDelete"),
  };
}

async function tally(parentId, childId) {
  const p = parentId ? await sinksFor(parentId) : [];
  const c = childId ? await sinksForChild(childId) : [];
  const a = countKinds(p);
  const b = countKinds(c);
  return {
    parentOn: a.parentOn,
    parentPost: a.parentPost,
    childOn: b.childOn,
    childPost: b.childPost,
    childDel: b.childDel,
  };
}

function sub(now, base) {
  const out = {};
  for (const k of Object.keys(now)) out[k] = now[k] - (base[k] || 0);
  return out;
}

async function settle(parentId, childId, base, maxMs) {
  const start = Date.now();
  let prev = null;
  let cur = sub(await tally(parentId, childId), base);
  while (Date.now() - start < maxMs) {
    await sleep(1000);
    prev = cur;
    cur = sub(await tally(parentId, childId), base);
    if (JSON.stringify(prev) === JSON.stringify(cur) && Date.now() - start >= 2000) return cur;
  }
  return cur;
}

async function revision(type, id) {
  if (!id) return { present: false, rev: null };
  const r = await call("GET", rec(type, id) + "?_fields=_rev");
  if (r.status !== 200) return { present: false, rev: null, status: r.status };
  return { present: true, rev: r.json?._rev ?? null, status: 200 };
}

function moved(before, after) {
  if (!before?.present || !after?.present) return null;
  return before.rev !== after.rev;
}

async function makeParent(rel) {
  return createParent({ mode: "children", rel, trace: true, postMode: "sink", name: `e21-${Date.now()}` });
}

async function makeChild(extra = {}) {
  const r = await call("POST", rec(CHILD) + "?_action=create", {
    body: { label: "k", value: 5, trace: true, ...extra },
  });
  return r;
}

function refValue(parentId, status = "active") {
  const value = { _ref: `managed/${PARENT}/${parentId}` };
  if (status != null) value._refProperties = { status };
  return value;
}

async function parentEdges(parentId, rel) {
  const r = await call("GET", `${rec(PARENT, parentId)}/${rel}?_queryFilter=true&_fields=_id,_ref,_refProperties`);
  if (r.status !== 200) return { status: r.status, edges: [], detail: scrub(JSON.stringify(r.json)?.slice(0, 180)) };
  return { status: 200, edges: r.json?.result || [] };
}

async function linkParent(parentId, rel, childId, status = "active") {
  const res = await call("PATCH", rec(PARENT, parentId), {
    body: [{
      operation: "add",
      field: `/${rel}/-`,
      value: { _ref: `managed/${CHILD}/${childId}`, _refProperties: { status } },
    }],
  });
  let edgeId = null;
  if (res.status === 200) {
    const got = await parentEdges(parentId, rel);
    const hit = got.edges.find((e) => String(e._ref || "").endsWith(`/${childId}`));
    edgeId = hit?._id || null;
  }
  return { res, edgeId };
}

async function setChildLink(childId, childRel, childMany, parentId, status = "active") {
  const value = refValue(parentId, status);
  if (childMany) {
    return call("PATCH", rec(CHILD, childId), {
      body: [{ operation: "add", field: `/${childRel}/-`, value }],
    });
  }
  return call("PATCH", rec(CHILD, childId), {
    body: [{ operation: "replace", field: `/${childRel}`, value }],
  });
}

async function patchEdgeFromChild(childId, childRel, childMany, edgeId, status) {
  const attempts = [];
  const note = (via, res) => attempts.push({
    via, status: res.status, ms: Math.round(res.ms), detail: scrub(JSON.stringify(res.json)?.slice(0, 160)),
  });
  if (childMany && edgeId) {
    const res = await call("PATCH", `${rec(CHILD, childId)}/${childRel}/${edgeId}`, {
      body: [{ operation: "replace", field: "/_refProperties/status", value: status }],
    });
    note("edge", res);
    if (res.status === 200) return { ok: true, via: "edge", http: res, attempts };
  }
  if (!childMany) {
    const res = await call("PATCH", `${rec(CHILD, childId)}/${childRel}`, {
      body: [{ operation: "replace", field: "/_refProperties/status", value: status }],
    });
    note("subresource", res);
    if (res.status === 200) return { ok: true, via: "subresource", http: res, attempts };
  }
  const res = await call("PATCH", rec(CHILD, childId), {
    body: [{ operation: "replace", field: `/${childRel}/_refProperties/status`, value: status }],
  });
  note("field", res);
  return { ok: res.status === 200, via: "field", http: res, attempts };
}

function httpOf(res) {
  if (!res) return null;
  return { status: res.status, ms: Math.round(res.ms), error: res.error || null };
}

async function positiveControl() {
  const p = await makeParent("children");
  const created = await makeChild();
  if (created.status !== 200 && created.status !== 201) {
    return { ok: false, reason: "child create", parentHttp: null, childHttp: created.status };
  }
  const c = created.json;
  const pr = await call("PATCH", rec(PARENT, p._id), {
    body: [{ operation: "replace", field: "/name", value: `control-${Date.now()}` }],
  });
  const cr = await call("PATCH", rec(CHILD, c._id), {
    body: [{ operation: "replace", field: "/label", value: `control-${Date.now()}` }],
  });
  await sleep(2000);
  const hooks = {
    parent: await tally(p._id, null),
    child: await tally(null, c._id),
  };
  const okParent = pr.status === 200 && hooks.parent.parentOn >= 1 && hooks.parent.parentPost >= 1;
  const okChild = cr.status === 200 && hooks.child.childOn >= 1 && hooks.child.childPost >= 1;
  return { ok: okParent && okChild, parentHttp: pr.status, childHttp: cr.status, hooks };
}

async function requireControl() {
  // A config/managed write makes the router 404 the POC types for a few
  // seconds. That is not a hook miss. Retry those. A 200 whose hooks stayed
  // quiet is a real miss, and one retry is enough to void the part.
  let last = null;
  for (let i = 0; i < 8; i++) {
    last = await positiveControl();
    console.log(`control ${last.ok ? "fired" : "MISSED"} parent ${last.parentHttp} child ${last.childHttp} ${last.reason || ""} ${JSON.stringify(last.hooks || {})}`);
    if (last.ok) return last;
    const quiet = last.parentHttp === 200 && last.childHttp === 200;
    if (quiet && i >= 1) break;
    await sleep(3000);
  }
  const err = new Error("positive control did not fire; this part is void");
  err.control = last;
  throw err;
}

// One measured write. `late` waits out to LATE_MS from the write.
async function measure(write, pair, { late = false } = {}) {
  const { parentRel, childRel, childMany } = pair;
  const row = { pair: pair.name, write, parentRel, childRel };
  const p = await makeParent(parentRel);
  let child = null;
  let dest = null;
  let edgeId = null;

  const needLink = ["replace-reverse", "child-plain", "child-delete", "edge-parent", "edge-child", "parent-plain", "parent-delete"].includes(write);
  if (write !== "create-reverse" && write !== "parent-add") {
    const created = await makeChild();
    if (created.status !== 200 && created.status !== 201) {
      return { ...row, http: created.status, error: "child create failed", detail: scrub(JSON.stringify(created.json)?.slice(0, 200)) };
    }
    child = created.json;
  }
  if (needLink) {
    const linked = await linkParent(p._id, parentRel, child._id, "active");
    edgeId = linked.edgeId;
    if (linked.res.status !== 200) {
      return { ...row, http: linked.res.status, error: "setup link failed", detail: scrub(JSON.stringify(linked.res.json)?.slice(0, 200)) };
    }
    await settle(p._id, child._id, { parentOn: 0, parentPost: 0, childOn: 0, childPost: 0, childDel: 0 }, WAIT_MS);
  }

  const ids = [["parent", PARENT, p._id]];
  if (child) ids.push(["child", CHILD, child._id]);
  if (write === "replace-reverse") {
    dest = await makeParent(parentRel);
    ids.push(["dest", PARENT, dest._id]);
  }
  const beforeRev = {};
  for (const [name, type, id] of ids) beforeRev[name] = await revision(type, id);
  const base = await tally(p._id, child?._id);
  const destBase = dest ? await tally(dest._id, null) : null;

  let res = null;
  let via = null;
  let attempts = null;
  if (write === "create-reverse") {
    const value = refValue(p._id, "active");
    const body = { label: "k", value: 5, trace: true };
    body[childRel] = childMany ? [value] : value;
    res = await call("POST", rec(CHILD) + "?_action=create", { body });
    if (res.status === 200 || res.status === 201) child = res.json;
    via = "create";
  } else if (write === "replace-reverse") {
    res = await setChildLink(child._id, childRel, childMany, dest._id, "moved");
    via = childMany ? "add" : "replace";
  } else if (write === "child-plain") {
    res = await call("PATCH", rec(CHILD, child._id), {
      body: [{ operation: "replace", field: "/value", value: 50 }],
    });
    via = "value";
  } else if (write === "child-delete") {
    res = await call("DELETE", rec(CHILD, child._id));
    via = "delete";
  } else if (write === "edge-parent") {
    if (!edgeId) {
      return { ...row, http: 0, error: "setup link returned no edge id" };
    }
    res = await call("PATCH", `${rec(PARENT, p._id)}/${parentRel}/${edgeId}`, {
      body: [{ operation: "replace", field: "/_refProperties/status", value: "inactive" }],
    });
    via = "parent-edge";
  } else if (write === "edge-child") {
    const patched = await patchEdgeFromChild(child._id, childRel, childMany, edgeId, "inactive");
    res = patched.http;
    via = patched.via;
    attempts = patched.attempts;
  } else if (write === "parent-add") {
    const created = await makeChild();
    if (created.status !== 200 && created.status !== 201) {
      return { ...row, http: created.status, error: "child create failed" };
    }
    child = created.json;
    const base2 = await tally(p._id, child._id);
    Object.assign(base, base2);
    beforeRev.child = await revision(CHILD, child._id);
    res = await call("PATCH", rec(PARENT, p._id), {
      body: [{ operation: "add", field: `/${parentRel}/-`, value: { _ref: `managed/${CHILD}/${child._id}`, _refProperties: { status: "active" } } }],
    });
    via = "parent-add";
  } else if (write === "parent-plain") {
    res = await call("PATCH", rec(PARENT, p._id), {
      body: [{ operation: "replace", field: "/name", value: `plain-${Date.now()}` }],
    });
    via = "name";
  } else if (write === "parent-delete") {
    res = await call("DELETE", rec(PARENT, p._id));
    via = "delete";
  } else {
    throw new Error(`unknown write ${write}`);
  }

  const t0 = Date.now();
  const immediate = sub(await tally(p._id, child?._id), base);
  const hooks = await settle(p._id, child?._id, base, WAIT_MS);
  let lateHooks = null;
  if (late) {
    const remain = LATE_MS - (Date.now() - t0);
    if (remain > 0) await sleep(remain);
    lateHooks = sub(await tally(p._id, child?._id), base);
  }
  const afterRev = {};
  for (const [name, type, id] of ids) afterRev[name] = await revision(type, id);
  if (write === "parent-add" && child) afterRev.child = await revision(CHILD, child._id);
  const destHooks = dest ? sub(await tally(dest._id, null), destBase) : null;
  const out = {
    ...row,
    ...httpOf(res),
    via,
    attempts,
    immediate,
    hooks,
    late: lateHooks,
    parentRevMoved: moved(beforeRev.parent, afterRev.parent),
    childRevMoved: moved(beforeRev.child, afterRev.child),
    destRevMoved: dest ? moved(beforeRev.dest, afterRev.dest) : null,
    parentDeleted: beforeRev.parent?.present && afterRev.parent && !afterRev.parent.present,
    childDeleted: beforeRev.child?.present && afterRev.child && !afterRev.child.present,
    destHooks,
    detail: res && res.status >= 400 ? scrub(JSON.stringify(res.json)?.slice(0, 200)) : undefined,
  };
  const h = out.hooks;
  console.log(`${pair.name} ${write} http ${out.status} ${out.ms}ms p ${h.parentOn}/${h.parentPost} c ${h.childOn}/${h.childPost}/${h.childDel} revP ${out.parentRevMoved} revC ${out.childRevMoved}${lateHooks ? ` late ${JSON.stringify(lateHooks)}` : ""}`);
  return out;
}

async function withFlags(part, modes, pairs, writes, { lateFor } = {}) {
  const rows = [];
  const controls = [];
  const shapes = [];
  for (const mode of modes) {
    const cfg = configure(mode.env);
    const shape = await liveShape().catch((e) => ({ error: scrub(e.message) }));
    shapes.push({ flags: mode.name, configure: cfg.ok ? "ok" : "failed", shape });
    if (!cfg.ok) {
      rows.push({ flags: mode.name, error: "configure failed", detail: cfg.output });
      console.log(`configure ${mode.name} failed`);
      save(part, { controls, rows, shapes, partial: true });
      continue;
    }
    await sleep(2000);
    let control;
    try {
      control = await requireControl();
    } catch (e) {
      controls.push({ flags: mode.name, ok: false, control: e.control || null });
      save(part, { controls, rows, shapes, void: mode.name });
      throw e;
    }
    controls.push({ flags: mode.name, ...control });
    for (const pair of pairs) {
      for (const write of writes) {
        const late = lateFor ? lateFor(mode, pair, write) : false;
        try {
          const row = await measure(write, pair, { late });
          rows.push({ flags: mode.name, ...row });
        } catch (e) {
          console.log(`${pair.name} ${write} threw ${scrub(e.message)}`);
          rows.push({ flags: mode.name, pair: pair.name, write, error: scrub(e.message) });
        }
      }
    }
    save(part, { controls, rows, shapes, partial: true });
  }
  return { controls, rows, shapes };
}

async function warm(pairs) {
  for (const pair of pairs) {
    let live = false;
    let last = null;
    for (let i = 0; i < 8 && !live; i++) {
      const p = await makeParent(pair.parentRel);
      const created = await makeChild();
      if (created.status !== 200 && created.status !== 201) {
        last = `child ${created.status}`;
        await sleep(3000);
        continue;
      }
      const linked = await linkParent(p._id, pair.parentRel, created.json._id, "active");
      last = `link ${linked.res.status} edge ${linked.edgeId ? "yes" : "no"}`;
      live = linked.res.status === 200 && !!linked.edgeId;
      if (!live) await sleep(3000);
    }
    console.log(`warm ${pair.name}: ${live ? "live" : "NOT LIVE"} (${last})`);
    if (!live) throw new Error(`pair ${pair.name} never went live (${last})`);
  }
}

function lateForTable(mode, pair, write) {
  return (mode.name === "default" && pair.name === "mapped" && write === "child-plain")
    || (mode.name === "NOTIFY=1" && pair.name === "mapped" && write === "edge-parent");
}

async function partTable() {
  await warm(MAIN);
  const data = await withFlags("table", [
    { name: "default", env: { NOTIFY: "", FLAG_AT: "node" } },
    { name: "NOTIFY=1", env: { NOTIFY: "1", FLAG_AT: "node" } },
    { name: "NOTIFY=2", env: { NOTIFY: "2", FLAG_AT: "node" } },
  ], MAIN, TABLE_WRITES, { lateFor: lateForTable });
  save("table", data);
  return data;
}

async function partPlacement() {
  // notify on the relationship node (the table) versus on resourceCollection,
  // which is where the tenant puts it. No notifyRelationships.
  const data = await withFlags("placement", [
    { name: "NOTIFY=1-collection", env: { NOTIFY: "1", FLAG_AT: "collection", FANOUT: "off" } },
    { name: "NOTIFY=2-collection", env: { NOTIFY: "2", FLAG_AT: "collection", FANOUT: "off" } },
  ], MAIN, ["replace-reverse", "child-plain", "child-delete", "edge-parent", "edge-child", "parent-add"]);
  save("placement", data);
  return data;
}

async function partFanout() {
  const shapes = [
    { name: "object-child", fanout: "object-child", writes: ["child-plain", "replace-reverse"] },
    { name: "object-parent", fanout: "object-parent", writes: ["parent-plain", "parent-add"] },
    { name: "field-child", fanout: "field-child", writes: ["child-plain", "replace-reverse"] },
    { name: "field-parent", fanout: "field-parent", writes: ["parent-plain", "parent-add"] },
    { name: "rel-child", fanout: "rel-child", writes: ["replace-reverse", "child-plain"] },
    { name: "rel-parent", fanout: "rel-parent", writes: ["parent-add", "parent-plain"] },
  ];
  const flagSets = [
    { name: "default", env: { NOTIFY: "", FLAG_AT: "node" } },
    { name: "NOTIFY=1-node", env: { NOTIFY: "1", FLAG_AT: "node" } },
    { name: "NOTIFY=2-node", env: { NOTIFY: "2", FLAG_AT: "node" } },
  ];
  const rows = [];
  const controls = [];
  const stored = [];
  if (process.env.E21_RESUME === "1") {
    try {
      const prev = JSON.parse(readFileSync(join(OUT, "E21-fanout.json"), "utf8"));
      const kept = new Set((prev.rows || []).map((r) => r.flags));
      rows.push(...(prev.rows || []));
      controls.push(...(prev.controls || []).filter((c) => kept.has(c.flags)));
      stored.push(...(prev.shapes || []).filter((s) => kept.has(s.flags)));
      console.log(`resume ${rows.length} rows, ${kept.size} configs`);
    } catch (e) {
      console.log(`resume found nothing (${scrub(e.message)})`);
    }
  }
  const done = new Set(rows.map((r) => r.flags));
  const configs = [];
  for (const shape of shapes) {
    for (const flags of flagSets) {
      configs.push({
        name: `${shape.name}/${flags.name}`,
        env: { ...flags.env, FANOUT: shape.fanout },
        writes: shape.writes,
        mirror: false,
      });
    }
    // Tenant puts notify on resourceCollection. alpha_role.temporalConstraints
    // names members from the field that changes; alpha_organization.parent
    // names its other relationships from the relationship itself.
    if (shape.fanout === "field-child" || shape.fanout === "field-parent" || shape.fanout === "rel-child" || shape.fanout === "rel-parent") {
      configs.push({
        name: `${shape.name}/NOTIFY=1-collection`,
        env: { NOTIFY: "1", FLAG_AT: "collection", FANOUT: shape.fanout },
        writes: shape.writes,
        mirror: shape.fanout === "field-child" || shape.fanout === "rel-child",
      });
    }
  }
  // Tight copy of alpha_role.temporalConstraints: notifyRelationships on the
  // changing field, notify only on the named relationship's resourceCollection.
  // alpha_user itself declares no notifyRelationships.
  configs.push({
    name: "field-child/role-mirror",
    env: {
      NOTIFY_SPEC: JSON.stringify({
        parent: { notify: true, at: "collection" },
        gparent: { notify: true, at: "collection" },
      }),
      FANOUT: "field-child",
    },
    writes: ["child-plain", "replace-reverse"],
    mirror: true,
  });
  // Tight copy of alpha_organization.parent: notifySelf on the relationship
  // node, notify on its resourceCollection, notifyRelationships on that same
  // relationship naming the link that points at the other object.
  configs.push({
    name: "rel-child/org-mirror",
    env: {
      NOTIFY_SPEC: JSON.stringify({
        parent: { notify: true, notifySelf: true, at: "collection" },
        gparent: { notify: true, notifySelf: true, at: "collection" },
      }),
      FANOUT: "rel-child",
    },
    writes: ["replace-reverse", "child-plain"],
    mirror: true,
  });
  for (const cfg of configs) {
    if (done.has(cfg.name)) {
      console.log(`skip ${cfg.name}`);
      continue;
    }
    // notifyRelationships naming its own relationship 404s that managed
    // resource. Measured in partShape, not by driving the matrix at a dead type.
    if (cfg.name.startsWith("rel-child") || cfg.name.startsWith("rel-parent")) {
      console.log(`skip self-ref ${cfg.name}`);
      continue;
    }
    const set = configure(cfg.env);
    const shape = await liveShape().catch((e) => ({ error: scrub(e.message) }));
    stored.push({ flags: cfg.name, mirror: cfg.mirror, configure: set.ok ? "ok" : "failed", shape });
    if (!set.ok) {
      rows.push({ flags: cfg.name, error: "configure failed", detail: set.output, mirror: cfg.mirror });
      console.log(`configure ${cfg.name} failed`);
      save("fanout", { controls, rows, shapes: stored, partial: true });
      continue;
    }
    await sleep(2000);
    try {
      const control = await requireControl();
      controls.push({ flags: cfg.name, ...control });
    } catch (e) {
      controls.push({ flags: cfg.name, ok: false, control: e.control || null });
      save("fanout", { controls, rows, shapes: stored, void: cfg.name });
      throw e;
    }
    for (const pair of MAIN) {
      for (const write of cfg.writes) {
        try {
          const row = await measure(write, pair);
          rows.push({ flags: cfg.name, mirror: cfg.mirror, ...row });
        } catch (e) {
          rows.push({ flags: cfg.name, pair: pair.name, write, error: scrub(e.message) });
        }
      }
    }
    save("fanout", { controls, rows, shapes: stored, partial: true });
  }
  const data = { controls, rows, shapes: stored };
  save("fanout", data);
  return data;
}

const ZERO = { parentOn: 0, parentPost: 0, childOn: 0, childPost: 0, childDel: 0 };

async function typeQuery(type) {
  const r = await call("GET", `/openidm/managed/${type}?_queryFilter=true&_pageSize=1`);
  return {
    status: r.status,
    detail: r.status === 200 ? null : scrub(JSON.stringify(r.json)?.slice(0, 180)),
  };
}

// A config/managed write 404s the resource for a few seconds. A self-referential
// notifyRelationships stayed 404 until the property was cleared. Wait long
// enough to tell those apart.
async function awaitType(type, want, tries = 8) {
  let last = null;
  for (let i = 0; i < tries; i++) {
    last = await typeQuery(type);
    if (last.status === want) return { ...last, tries: i + 1 };
    await sleep(3000);
  }
  return { ...last, tries };
}

async function clearFanoutLive() {
  const cleared = configure({ FANOUT: "off", NOTIFY: "", NOTIFY_SPEC: "", FLAG_AT: "node", RETURN_BY_DEFAULT: "0" });
  const child = await awaitType(CHILD, 200, 8);
  const parent = await awaitType(PARENT, 200, 8);
  return { cleared: cleared.ok, child, parent };
}

// Apply one fanout, run `fn`, then always clear it. A type that does not come
// back is a failed part: later measurements would be against a dead resource.
async function withFanout(env, fn) {
  const set = configure(env);
  let result = { configure: set.ok ? "ok" : "failed" };
  try {
    const shape = await liveShape().catch((e) => ({ error: scrub(e.message) }));
    result.shape = shape;
    if (!set.ok) {
      result.detail = set.output;
      return result;
    }
    await sleep(2000);
    const measured = await fn();
    result = { ...result, ...measured };
    return result;
  } finally {
    const restored = await clearFanoutLive();
    result.restored = {
      configure: restored.cleared,
      child: restored.child.status,
      parent: restored.parent.status,
    };
    if (restored.child.status !== 200 || restored.parent.status !== 200) {
      const err = new Error(`POC types stayed down after clearing fanout (child ${restored.child.status}, parent ${restored.parent.status})`);
      err.result = result;
      throw err;
    }
  }
}

// Change one has-one on the child. `watchRel` is the other relationship, already
// linked to `far`. The question is whether that far parent is notified.
async function changeOtherRel(changedRel, watchRel) {
  const far = await makeParent(watchRel);
  const created = await makeChild();
  if (created.status !== 200 && created.status !== 201) {
    return { write: `change-${changedRel}`, watch: watchRel, error: "child create", http: created.status };
  }
  const child = created.json;
  const linked = await linkParent(far._id, watchRel, child._id, "active");
  if (linked.res.status !== 200) {
    return { write: `change-${changedRel}`, watch: watchRel, error: "watch link", http: linked.res.status };
  }
  await settle(far._id, child._id, ZERO, WAIT_MS);
  const nearRel = changedRel === "parent" ? "children" : "gchildren";
  const near = await makeParent(nearRel);
  const beforeFar = await revision(PARENT, far._id);
  const beforeNear = await revision(PARENT, near._id);
  const beforeChild = await revision(CHILD, child._id);
  const baseFar = await tally(far._id, null);
  const baseNear = await tally(near._id, null);
  const baseChild = await tally(null, child._id);
  const res = await setChildLink(child._id, changedRel, false, near._id, "moved");
  const farHooks = await settle(far._id, null, baseFar, WAIT_MS);
  const nearHooks = sub(await tally(near._id, null), baseNear);
  const childHooks = sub(await tally(null, child._id), baseChild);
  const out = {
    write: `change-${changedRel}`,
    watch: watchRel,
    http: res.status,
    ms: Math.round(res.ms),
    farHooks,
    nearHooks,
    childHooks,
    farRev: moved(beforeFar, await revision(PARENT, far._id)),
    nearRev: moved(beforeNear, await revision(PARENT, near._id)),
    childRev: moved(beforeChild, await revision(CHILD, child._id)),
    detail: res.status >= 400 ? scrub(JSON.stringify(res.json)?.slice(0, 180)) : undefined,
  };
  console.log(`shape ${out.write} watch ${watchRel} http ${out.http} far ${farHooks.parentOn}/${farHooks.parentPost} near ${nearHooks.parentOn}/${nearHooks.parentPost} child ${childHooks.childOn}/${childHooks.childPost} farRev ${out.farRev}`);
  return out;
}

// Same linkage, then a plain field. Names on the relationship must not fire.
async function plainWhileLinked(watchRel) {
  const far = await makeParent(watchRel);
  const created = await makeChild();
  if (created.status !== 200 && created.status !== 201) {
    return { write: "child-plain", watch: watchRel, error: "child create", http: created.status };
  }
  const child = created.json;
  const linked = await linkParent(far._id, watchRel, child._id, "active");
  if (linked.res.status !== 200) {
    return { write: "child-plain", watch: watchRel, error: "watch link", http: linked.res.status };
  }
  await settle(far._id, child._id, ZERO, WAIT_MS);
  const beforeFar = await revision(PARENT, far._id);
  const beforeChild = await revision(CHILD, child._id);
  const baseFar = await tally(far._id, null);
  const baseChild = await tally(null, child._id);
  const res = await call("PATCH", rec(CHILD, child._id), {
    body: [{ operation: "replace", field: "/value", value: 50 }],
  });
  const farHooks = await settle(far._id, null, baseFar, WAIT_MS);
  const childHooks = sub(await tally(null, child._id), baseChild);
  const out = {
    write: "child-plain",
    watch: watchRel,
    http: res.status,
    ms: Math.round(res.ms),
    farHooks,
    childHooks,
    farRev: moved(beforeFar, await revision(PARENT, far._id)),
    childRev: moved(beforeChild, await revision(CHILD, child._id)),
  };
  console.log(`shape child-plain watch ${watchRel} http ${out.http} far ${farHooks.parentOn}/${farHooks.parentPost} child ${childHooks.childOn}/${childHooks.childPost} farRev ${out.farRev}`);
  return out;
}

// Change one has-many on the parent. `watchRel` already links `farChild`.
async function changeOtherParentRel(changedRel, watchRel) {
  const parent = await makeParent("children");
  const created = await makeChild();
  if (created.status !== 200 && created.status !== 201) {
    return { write: `add-${changedRel}`, watch: watchRel, error: "child create", http: created.status };
  }
  const farChild = created.json;
  const linked = await linkParent(parent._id, watchRel, farChild._id, "active");
  if (linked.res.status !== 200) {
    return { write: `add-${changedRel}`, watch: watchRel, error: "watch link", http: linked.res.status };
  }
  await settle(parent._id, farChild._id, ZERO, WAIT_MS);
  const nearCreated = await makeChild();
  if (nearCreated.status !== 200 && nearCreated.status !== 201) {
    return { write: `add-${changedRel}`, watch: watchRel, error: "near child create", http: nearCreated.status };
  }
  const beforeFar = await revision(CHILD, farChild._id);
  const beforeParent = await revision(PARENT, parent._id);
  const baseFar = await tally(null, farChild._id);
  const baseParent = await tally(parent._id, null);
  const res = await linkParent(parent._id, changedRel, nearCreated.json._id, "active");
  const farHooks = await settle(null, farChild._id, baseFar, WAIT_MS);
  const parentHooks = sub(await tally(parent._id, null), baseParent);
  const out = {
    write: `add-${changedRel}`,
    watch: watchRel,
    http: res.res.status,
    ms: Math.round(res.res.ms),
    farHooks,
    parentHooks,
    farRev: moved(beforeFar, await revision(CHILD, farChild._id)),
    parentRev: moved(beforeParent, await revision(PARENT, parent._id)),
    detail: res.res.status >= 400 ? scrub(JSON.stringify(res.res.json)?.slice(0, 180)) : undefined,
  };
  console.log(`shape ${out.write} watch ${watchRel} http ${out.http} farChild ${farHooks.childOn}/${farHooks.childPost} parent ${parentHooks.parentOn}/${parentHooks.parentPost} farRev ${out.farRev}`);
  return out;
}

async function parentPlainWhileLinked(watchRel) {
  const parent = await makeParent("children");
  const created = await makeChild();
  if (created.status !== 200 && created.status !== 201) {
    return { write: "parent-plain", watch: watchRel, error: "child create", http: created.status };
  }
  const farChild = created.json;
  const linked = await linkParent(parent._id, watchRel, farChild._id, "active");
  if (linked.res.status !== 200) {
    return { write: "parent-plain", watch: watchRel, error: "watch link", http: linked.res.status };
  }
  await settle(parent._id, farChild._id, ZERO, WAIT_MS);
  const beforeFar = await revision(CHILD, farChild._id);
  const baseFar = await tally(null, farChild._id);
  const baseParent = await tally(parent._id, null);
  const res = await call("PATCH", rec(PARENT, parent._id), {
    body: [{ operation: "replace", field: "/name", value: `plain-${Date.now()}` }],
  });
  const farHooks = await settle(null, farChild._id, baseFar, WAIT_MS);
  const parentHooks = sub(await tally(parent._id, null), baseParent);
  const out = {
    write: "parent-plain",
    watch: watchRel,
    http: res.status,
    ms: Math.round(res.ms),
    farHooks,
    parentHooks,
    farRev: moved(beforeFar, await revision(CHILD, farChild._id)),
  };
  console.log(`shape parent-plain watch ${watchRel} http ${out.http} farChild ${farHooks.childOn}/${farHooks.childPost} parent ${parentHooks.parentOn}/${parentHooks.parentPost}`);
  return out;
}

async function relOtherChild(label, env) {
  return withFanout(env, async () => {
    const child = await awaitType(CHILD, 200, 8);
    const parent = await typeQuery(PARENT);
    if (child.status !== 200 || parent.status !== 200) {
      console.log(`shape ${label} offline child ${child.status} parent ${parent.status}`);
      return { label, offline: true, child, parent };
    }
    const control = await requireControl();
    const rows = [];
    for (const fn of [
      () => changeOtherRel("parent", "gchildren"),
      () => plainWhileLinked("gchildren"),
      () => changeOtherRel("gparent", "children"),
    ]) {
      try {
        rows.push(await fn());
      } catch (e) {
        rows.push({ error: scrub(e.message) });
      }
    }
    return { label, offline: false, control: { ok: control.ok, parentHttp: control.parentHttp, childHttp: control.childHttp }, rows };
  });
}

async function relOtherParent(label, env) {
  return withFanout(env, async () => {
    const parent = await awaitType(PARENT, 200, 8);
    const child = await typeQuery(CHILD);
    if (child.status !== 200 || parent.status !== 200) {
      console.log(`shape ${label} offline child ${child.status} parent ${parent.status}`);
      return { label, offline: true, child, parent };
    }
    const control = await requireControl();
    const rows = [];
    for (const fn of [
      () => changeOtherParentRel("children", "gchildren"),
      () => parentPlainWhileLinked("gchildren"),
      () => changeOtherParentRel("gchildren", "children"),
    ]) {
      try {
        rows.push(await fn());
      } catch (e) {
        rows.push({ error: scrub(e.message) });
      }
    }
    return { label, offline: false, control: { ok: control.ok, parentHttp: control.parentHttp, childHttp: control.childHttp }, rows };
  });
}

// Self-naming notifyRelationships took the resource offline in the fanout run.
// Probe it once per end, always clear it, then measure the tenant's shape:
// the relationship names the OTHER relationship (alpha_organization.parent
// names children, not itself).
async function partShape() {
  const rows = [];
  const record = async (label, fn) => {
    try {
      rows.push(await fn());
      save("shape", { rows, partial: true });
    } catch (e) {
      rows.push({ label, error: scrub(e.message), control: e.control || null });
      save("shape", { rows, partial: true, void: label });
      throw e;
    }
  };
  await record("self-ref-child", () => withFanout({ FANOUT: "rel-child", NOTIFY: "" }, async () => {
    const during = await awaitType(CHILD, 200, 8);
    const other = await typeQuery(PARENT);
    console.log(`shape self-ref child ${during.status} after ${during.tries} parent ${other.status}`);
    return { label: "self-ref-child", during, other };
  }));
  await record("self-ref-parent", () => withFanout({ FANOUT: "rel-parent", NOTIFY: "" }, async () => {
    const during = await awaitType(PARENT, 200, 8);
    const other = await typeQuery(CHILD);
    console.log(`shape self-ref parent ${during.status} after ${during.tries} child ${other.status}`);
    return { label: "self-ref-parent", during, other };
  }));
  await record("rel-other-child", () => relOtherChild("rel-other-child", { FANOUT: "rel-other-child", NOTIFY: "" }));
  await record("rel-other-child/org-mirror", () => relOtherChild("rel-other-child/org-mirror", {
    FANOUT: "rel-other-child",
    NOTIFY_SPEC: JSON.stringify({
      parent: { notify: true, notifySelf: true, at: "collection" },
      gparent: { notify: true, notifySelf: true, at: "collection" },
    }),
  }));
  await record("rel-other-parent", () => relOtherParent("rel-other-parent", { FANOUT: "rel-other-parent", NOTIFY: "" }));
  const data = { rows };
  save("shape", data);
  return data;
}

async function partDeleteWait() {
  // Round 1 reported a parent hook on child delete under NOTIFY=1. The table
  // waited 6s and saw none. Wait 30s, then once more with the child's scripts
  // removed, in case onDelete is what suppresses it.
  const cfg = configure({ NOTIFY: "1", FLAG_AT: "node", FANOUT: "off", RETURN_BY_DEFAULT: "0" });
  if (!cfg.ok) throw new Error(`deletewait configure failed: ${cfg.output}`);
  await sleep(2000);
  const shape = await liveShape();
  await requireControl();
  const withHooks = [];
  for (const pair of MAIN) withHooks.push(await measure("child-delete", pair, { late: true }));
  const strip = runSetup("strip-child-hooks");
  let bare = { strip };
  if (strip.ok) {
    await sleep(2000);
    bare.row = await measure("child-delete", MAIN[0], { late: true });
    bare.restore = runSetup("hooks");
  }
  const data = { shape, withHooks, bare };
  save("deletewait", data);
  return data;
}

async function partPeers() {
  await warm([PEERS]);
  const data = await withFlags("peers", [
    { name: "default", env: { NOTIFY: "", FLAG_AT: "node" } },
    { name: "NOTIFY=1", env: { NOTIFY: "1", FLAG_AT: "node" } },
    { name: "NOTIFY=2", env: { NOTIFY: "2", FLAG_AT: "node" } },
  ], [PEERS], ["create-reverse", "replace-reverse", "child-plain", "child-delete", "edge-parent", "edge-child", "parent-add", "parent-plain", "parent-delete"]);
  save("peers", data);
  return data;
}

async function partOwner() {
  await warm([OWNER]);
  const specs = [
    { name: "none", spec: {} },
    { name: "forward-notify-node", spec: { owner: { notify: true, at: "node" } } },
    { name: "forward-notify-collection", spec: { owner: { notify: true, at: "collection" } } },
    { name: "forward-notifySelf", spec: { owner: { notifySelf: true } } },
    { name: "reverse-notify-collection", spec: { owned: { notify: true, at: "collection" } } },
    { name: "reverse-notifySelf", spec: { owned: { notifySelf: true } } },
    { name: "both-ends-tenant", spec: {
      owner: { notify: true, notifySelf: true, at: "collection" },
      owned: { notify: true, notifySelf: true, at: "collection" },
    } },
  ];
  const writes = ["create-reverse", "replace-reverse", "child-plain", "parent-plain", "edge-parent", "edge-child", "parent-add", "child-delete"];
  const modes = [
    ...specs.map((s) => ({ name: s.name, env: { NOTIFY_SPEC: JSON.stringify(s.spec), FANOUT: "off" } })),
    { name: "field-owner+forward-notify-collection", env: {
      NOTIFY_SPEC: JSON.stringify({ owner: { notify: true, at: "collection" } }),
      FANOUT: "field-owner",
    } },
  ];
  const data = await withFlags("owner", modes, [OWNER], writes);
  save("owner", data);
  return data;
}

function relSummary(value) {
  if (value == null) return null;
  const edge = (e) => {
    if (!e || typeof e !== "object") return null;
    const rp = e._refProperties || {};
    return { refPropKeys: Object.keys(rp).sort(), status: Object.prototype.hasOwnProperty.call(rp, "status") ? rp.status : null };
  };
  if (Array.isArray(value)) return { kind: "array", n: value.length, first: value[0] ? edge(value[0]) : null };
  if (value.result) return { kind: "query", n: value.result.length, first: value.result[0] ? edge(value.result[0]) : null };
  if (typeof value === "object") return { kind: "object", edge: edge(value._ref ? value : value) };
  return { kind: typeof value };
}

function keysOf(body) {
  if (!body || typeof body !== "object") return [];
  return Object.keys(body).sort();
}

async function newestNote(parentId, childId, kind) {
  const rows = kind.startsWith("child-") ? await sinksForChild(childId) : await sinksFor(parentId);
  const hits = rows.filter((s) => s.kind === kind).sort((a, b) => (a.at || 0) - (b.at || 0));
  const sink = hits[hits.length - 1];
  if (!sink?.note) return null;
  try {
    return JSON.parse(sink.note);
  } catch {
    return { raw: "unparsed" };
  }
}

async function partAttrs() {
  const rows = [];
  for (const pair of MAIN) {
    for (const ret of ["0", "1"]) {
      const cfg = configure({ NOTIFY: "", FANOUT: "off", RETURN_BY_DEFAULT: ret });
      if (!cfg.ok) {
        rows.push({ pair: pair.name, returnByDefault: ret, error: "configure failed", detail: cfg.output });
        continue;
      }
      await sleep(2000);
      if (ret === "0") await requireControl();
      const p = await makeParent(pair.parentRel);
      const created = await makeChild();
      const c = created.json;
      const linked = await linkParent(p._id, pair.parentRel, c._id, "active");
      await settle(p._id, c._id, { parentOn: 0, parentPost: 0, childOn: 0, childPost: 0, childDel: 0 }, WAIT_MS);
      const parentEdge = linked.edgeId
        ? await call("GET", `${rec(PARENT, p._id)}/${pair.parentRel}/${linked.edgeId}`)
        : null;
      const parentList = await call("GET", `${rec(PARENT, p._id)}/${pair.parentRel}?_queryFilter=true&_fields=*`);
      const childSub = await call("GET", `${rec(CHILD, c._id)}/${pair.childRel}${pair.childMany ? "?_queryFilter=true&_fields=*" : ""}`);
      const childFields = await call("GET", `${rec(CHILD, c._id)}?_fields=${pair.childRel}`);
      const parentGet = await call("GET", rec(PARENT, p._id));
      const childGet = await call("GET", rec(CHILD, c._id));
      const beforeP = await revision(PARENT, p._id);
      const beforeC = await revision(CHILD, c._id);
      const fromChild = await patchEdgeFromChild(c._id, pair.childRel, pair.childMany, linked.edgeId, "from-child");
      const afterChildWriteP = await revision(PARENT, p._id);
      const afterChildWriteC = await revision(CHILD, c._id);
      const parentEdgeAfter = linked.edgeId
        ? await call("GET", `${rec(PARENT, p._id)}/${pair.parentRel}/${linked.edgeId}`)
        : null;
      const beforeParentEdge = await revision(PARENT, p._id);
      const parentEdgeWrite = linked.edgeId
        ? await call("PATCH", `${rec(PARENT, p._id)}/${pair.parentRel}/${linked.edgeId}`, {
          body: [{ operation: "replace", field: "/_refProperties/status", value: "from-parent" }],
        })
        : null;
      const afterParentEdge = await revision(PARENT, p._id);
      const childAfterParentEdge = await revision(CHILD, c._id);
      const childSubAfterParent = await call("GET", `${rec(CHILD, c._id)}/${pair.childRel}${pair.childMany ? "?_queryFilter=true&_fields=*" : ""}`);
      const childFieldsAfterParent = await call("GET", `${rec(CHILD, c._id)}?_fields=${pair.childRel}`);
      await settle(p._id, c._id, { parentOn: 0, parentPost: 0, childOn: 0, childPost: 0, childDel: 0 }, 2000);
      const base = await tally(p._id, c._id);
      await call("PATCH", rec(CHILD, c._id), { body: [{ operation: "replace", field: "/label", value: `snap-${ret}` }] });
      await call("PATCH", rec(PARENT, p._id), { body: [{ operation: "replace", field: "/counter", value: Number(ret) }] });
      await settle(p._id, c._id, base, WAIT_MS);
      const childSnap = await newestNote(p._id, c._id, "child-onUpdate");
      const parentSnap = await newestNote(p._id, c._id, "onUpdate");
      rows.push({
        pair: pair.name,
        returnByDefault: ret,
        link: linked.res.status,
        reads: {
          parentEdge: parentEdge ? { status: parentEdge.status, summary: relSummary(parentEdge.json) } : null,
          parentList: { status: parentList.status, summary: relSummary(parentList.json) },
          childSub: { status: childSub.status, summary: relSummary(childSub.json) },
          childFields: { status: childFields.status, keys: keysOf(childFields.json), summary: relSummary(childFields.json?.[pair.childRel]) },
          parentGetKeys: parentGet.status === 200 ? keysOf(parentGet.json) : parentGet.status,
          childGetKeys: childGet.status === 200 ? keysOf(childGet.json) : childGet.status,
        },
        writeFromChild: { via: fromChild.via, ok: fromChild.ok, attempts: fromChild.attempts },
        parentRevOnChildEdgeWrite: moved(beforeP, afterChildWriteP),
        childRevOnChildEdgeWrite: moved(beforeC, afterChildWriteC),
        statusSeenFromParentAfterChildWrite: relSummary(parentEdgeAfter?.json),
        parentEdgeWrite: parentEdgeWrite ? parentEdgeWrite.status : null,
        parentRevOnParentEdgeWrite: moved(beforeParentEdge, afterParentEdge),
        childRevOnParentEdgeWrite: moved(afterChildWriteC, childAfterParentEdge),
        childSeesParentEdgeWrite: {
          sub: { status: childSubAfterParent.status, summary: relSummary(childSubAfterParent.json) },
          fields: { status: childFieldsAfterParent.status, summary: relSummary(childFieldsAfterParent.json?.[pair.childRel]) },
        },
        childSnap,
        parentSnap,
      });
      console.log(`attrs ${pair.name} returnByDefault ${ret} childWrite ${fromChild.via} ${fromChild.ok} parentRev ${moved(beforeP, afterChildWriteP)} childRev ${moved(beforeC, afterChildWriteC)}`);
    }
  }
  configure({ RETURN_BY_DEFAULT: "0", NOTIFY: "", FANOUT: "off" });
  save("attrs", { rows });
  return { rows };
}

async function partTrace() {
  const cfg = configure({ NOTIFY: "1", FLAG_AT: "node", FANOUT: "off", RETURN_BY_DEFAULT: "0" });
  if (!cfg.ok) throw new Error(`trace configure failed: ${cfg.output}`);
  await sleep(2000);
  await requireControl();
  await warm([MAIN[0]]);
  const runs = [];
  for (const flags of ["on", "off"]) {
    if (flags === "off") {
      const cleared = configure({ NOTIFY: "", FLAG_AT: "node", FANOUT: "off" });
      if (!cleared.ok) throw new Error(`trace clear failed: ${cleared.output}`);
      await sleep(2000);
      await requireControl();
    }
    const samples = [];
    for (let i = 0; i < 3; i++) {
      const p = await makeParent("children");
      const created = await makeChild();
      const c = created.json;
      const linked = await linkParent(p._id, "children", c._id, "active");
      await settle(p._id, c._id, { parentOn: 0, parentPost: 0, childOn: 0, childPost: 0, childDel: 0 }, WAIT_MS);
      const tx = `e21-${randomUUID()}`;
      const base = await tally(p._id, c._id);
      const seen = new Set([...(await sinksFor(p._id)), ...(await sinksForChild(c._id))].map((s) => s._id));
      const res = await call("PATCH", `${rec(PARENT, p._id)}/children/${linked.edgeId}`, {
        body: [{ operation: "replace", field: "/_refProperties/status", value: "inactive" }],
        tx,
      });
      const immediate = sub(await tally(p._id, c._id), base);
      const hooks = await settle(p._id, c._id, base, WAIT_MS);
      const fresh = [...(await sinksFor(p._id)), ...(await sinksForChild(c._id))].filter((s) => !seen.has(s._id));
      const txids = [...new Set(fresh.map((s) => s.txid).filter(Boolean))];
      samples.push({
        http: res.status,
        ms: Math.round(res.ms),
        immediate,
        hooks,
        txidsRelate: txids.map((id) => (id === tx ? "exact" : id.startsWith(tx) ? "prefix" : "other")),
        txidCount: txids.length,
      });
      console.log(`trace flags ${flags} #${i} ${res.status} ${Math.round(res.ms)}ms immediate ${JSON.stringify(immediate)} tx ${samples.at(-1).txidsRelate.join(",") || "none"}`);
    }
    let log = null;
    const tx = `e21-${randomUUID()}`;
    const p = await makeParent("children");
    const created = await makeChild();
    const linked = await linkParent(p._id, "children", created.json._id, "active");
    await settle(p._id, created.json._id, { parentOn: 0, parentPost: 0, childOn: 0, childPost: 0, childDel: 0 }, WAIT_MS);
    const res = await call("PATCH", `${rec(PARENT, p._id)}/children/${linked.edgeId}`, {
      body: [{ operation: "replace", field: "/_refProperties/status", value: "inactive" }],
      tx,
    });
    await sleep(2000);
    try {
      const summary = fetchSummary(tx);
      log = summary.map((row) => ({
        relation: row.transactionId === tx ? "exact" : row.transactionId.startsWith(tx) ? "prefix" : "other",
        access: row.access,
        writes: row.writes.map((w) => ({ operation: w.operation, type: w.type, status: w.status })),
      }));
    } catch (e) {
      log = { error: scrub(e.message) };
    }
    runs.push({ flags, samples, log, logHttp: res.status, logMs: Math.round(res.ms) });
  }
  save("trace", { runs });
  return { runs };
}

async function partCas() {
  const out = [];
  for (const pair of MAIN) {
    const cfg = configure({ NOTIFY: "1", FLAG_AT: "node", FANOUT: "off", RETURN_BY_DEFAULT: "0" });
    if (!cfg.ok) throw new Error(`cas configure failed: ${cfg.output}`);
    await sleep(2000);
    await requireControl();
    const p = await makeParent(pair.parentRel);
    const kids = [];
    for (let i = 0; i < 10; i++) {
      const created = await makeChild({ value: i + 1 });
      if (created.status !== 200 && created.status !== 201) throw new Error(`cas child ${created.status}`);
      const linked = await linkParent(p._id, pair.parentRel, created.json._id, "active");
      if (linked.res.status !== 200 || !linked.edgeId) throw new Error(`cas link ${linked.res.status}`);
      kids.push({ id: created.json._id, edgeId: linked.edgeId });
    }
    await settle(p._id, null, { parentOn: 0, parentPost: 0, childOn: 0, childPost: 0, childDel: 0 }, WAIT_MS);
    const before = await read(PARENT, p._id);
    const baseSinks = await sinksFor(p._id);
    const seen = new Set(baseSinks.map((s) => s._id));
    const results = await burst(10, (i) => call("PATCH", `${rec(PARENT, p._id)}/${pair.parentRel}/${kids[i].edgeId}`, {
      body: [{ operation: "replace", field: "/_refProperties/status", value: "inactive" }],
    }));
    const immediateSinks = await sinksFor(p._id);
    const immediate = immediateSinks.filter((s) => s.kind === "onUpdate" && !seen.has(s._id)).length;
    await sleep(WAIT_MS);
    const sinks = await sinksFor(p._id);
    const on = sinks.filter((s) => s.kind === "onUpdate" && !seen.has(s._id));
    const post = sinks.filter((s) => s.kind === "postUpdate" && !seen.has(s._id)).length;
    const ordered = on.slice().sort((a, b) => (a.at || 0) - (b.at || 0));
    const repeatedBase = ordered.some((r, i) => i > 0 && r.oldRev && r.oldRev === ordered[i - 1].oldRev);
    const byOps = {};
    for (const s of on) byOps[s.ops || "(none)"] = (byOps[s.ops || "(none)"] || 0) + 1;
    const after = await read(PARENT, p._id);
    const edges = await parentEdges(p._id, pair.parentRel);
    const statuses = edges.edges.map((e) => e._refProperties?.status ?? null);
    const row = {
      pair: pair.name,
      http: summarise(results),
      immediateOnUpdate: immediate,
      onUpdateRuns: on.length,
      postUpdateRuns: post,
      distinctOldRev: new Set(on.map((s) => s.oldRev)).size,
      repeatedConsecutiveOldRev: repeatedBase,
      maxSameOps: Math.max(0, ...Object.values(byOps)),
      distinctOps: Object.keys(byOps).length,
      derivedBefore: before.derived || null,
      derivedAfter: after.derived || null,
      edgeStatuses: statuses,
      want: { count: 10, sum: 55, active: 0 },
    };
    console.log(`cas ${pair.name} ${JSON.stringify(row.http.byStatus)} on ${row.onUpdateRuns} post ${row.postUpdateRuns} derived ${JSON.stringify(row.derivedAfter)}`);
    out.push(row);
  }
  save("cas", { rows: out });
  return { rows: out };
}

async function partRepo() {
  const r = await call("GET", "/openidm/repo/relationships?_queryFilter=true&_pageSize=1");
  const data = { status: r.status, body: scrub(JSON.stringify(r.json)?.slice(0, 240)) };
  console.log(`repo/relationships ${data.status} ${data.body}`);
  save("repo", data);
  return data;
}

const PARTS = {
  repo: partRepo,
  table: partTable,
  deletewait: partDeleteWait,
  placement: partPlacement,
  fanout: partFanout,
  shape: partShape,
  peers: partPeers,
  owner: partOwner,
  attrs: partAttrs,
  trace: partTrace,
  cas: partCas,
};

const asked = process.argv.slice(2);
const list = asked.length === 0 || asked.includes("all")
  ? ["repo", "table", "deletewait", "placement", "fanout", "shape", "peers", "owner", "attrs", "trace", "cas"]
  : asked;
for (const name of list) {
  if (!PARTS[name]) {
    console.error(`unknown part ${name}; have ${Object.keys(PARTS).join(" ")} all`);
    process.exit(2);
  }
}
for (const name of list) {
  const t0 = Date.now();
  console.log(`== E21 ${name}`);
  try {
    await PARTS[name]();
    console.log(`== E21 ${name} done (${Math.round((Date.now() - t0) / 1000)}s)`);
  } catch (e) {
    console.error(`== E21 ${name} FAILED (${Math.round((Date.now() - t0) / 1000)}s): ${scrub(e.message)}`);
    if (e.control) save(`${name}-void`, { control: e.control, error: scrub(e.message) });
    process.exitCode = 1;
    break;
  }
}
