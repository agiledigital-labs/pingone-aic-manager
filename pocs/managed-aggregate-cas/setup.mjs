// Install / remove the POC's throwaway managed types.
//
//   node setup.mjs preflight  compare live config with the round-1 backups
//   node setup.mjs install    backup, create types, relationship, schema+hooks
//   node setup.mjs hooks      re-push the hook sources only
//   node setup.mjs configure  flags, notifyRelationships, returnByDefault
//   node setup.mjs teardown   delete records and types, compare with backup
//
// The backup and every config/managed snapshot go to .state/ (gitignored):
// they hold the whole tenant schema. Round 2 must not overwrite
// managed-before.json or repo.ds-before.json. The repo.ds success condition
// is genericMapping equal to .state/run1/repo.ds-after.json (the pre-POC
// baseline plus the two orphan mappings round 1 could not remove).

import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync, appendFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { ok, call, sleep, PROJECT, PARENT, CHILD, SINK, POC_TYPES, deleteAll } from "./lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const STATE = join(HERE, ".state");
mkdirSync(STATE, { recursive: true });
const BACKUP = join(STATE, "managed-before.json");
const REPO_BEFORE = join(STATE, "repo.ds-before.json");
const REPO_ORPHANS = join(STATE, "run1/repo.ds-after.json");

const PARENT_RELS = ["children", "gchildren", "peers", "owned"];
const CHILD_RELS = ["parent", "gparent", "peerOf", "owner"];

function aic(...args) {
  const r = spawnSync(process.env.AIC || "aic", ["--no-prompt", ...args], {
    encoding: "utf8",
    env: { ...process.env, AIC_PROJECT: PROJECT },
    maxBuffer: 8 * 1024 * 1024,
  });
  const text = `--- aic ${args.join(" ")} (status ${r.status})\n${r.stdout || ""}${r.stderr || ""}`;
  appendFileSync(join(STATE, "round2-aic.log"), text);
  if (r.stdout) process.stdout.write(r.stdout);
  if (r.stderr) process.stderr.write(r.stderr);
  if (r.error) throw r.error;
  if (r.status !== 0) {
    const err = new Error(`Command failed: aic ${args.join(" ")}`);
    err.stdout = r.stdout;
    err.stderr = r.stderr;
    throw err;
  }
  return r.stdout || "";
}

const str = (extra = {}) => ({ type: "string", searchable: false, viewable: true, userEditable: true, ...extra });
const num = (extra = {}) => ({ type: "number", searchable: false, viewable: true, userEditable: true, ...extra });
const bool = () => ({ type: "boolean", searchable: false, viewable: true, userEditable: true });
const obj = () => ({ type: "object", searchable: false, viewable: true, userEditable: true });
const arr = () => ({ type: "array", items: { type: "object" }, searchable: false, viewable: true, userEditable: true });

const FIELDS = {
  [PARENT]: {
    name: str({ searchable: true }),
    mode: str(),
    postMode: str(),
    trace: bool(),
    threshold: num(),
    counter: num(),
    inbox: arr(),
    entries: arr(),
    derived: obj(),
    audit: arr(),
    auditSeq: num(),
    data: obj(),
    items: arr(),
    rel: str(),
  },
  [CHILD]: { label: str({ searchable: true }), value: num(), rank: num(), trace: bool() },
  [SINK]: {
    kind: str({ searchable: true }),
    parent: str({ searchable: true }),
    child: str({ searchable: true }),
    runId: str(),
    rev: str(),
    oldRev: str(),
    outcome: str(),
    count: num(),
    inboxLen: num(),
    oldCount: num(),
    ops: str(),
    note: str(),
    txid: str({ searchable: true }),
    at: num(),
  },
};

const hook = (file) => ({ type: "text/javascript", source: readFileSync(join(HERE, "hooks", file), "utf8") });

const managed = () => ok("GET", "/openidm/config/managed");
const others = (doc) => doc.objects.filter((o) => !POC_TYPES.includes(o.name));
const stable = (v) => JSON.stringify(v);
const gm = (d) => d.resourceMapping.genericMapping;

function requireBackups() {
  for (const path of [BACKUP, REPO_BEFORE, REPO_ORPHANS]) {
    if (!existsSync(path)) throw new Error(`missing ${path}; refusing to overwrite or invent a baseline`);
  }
}

// Live config/managed and genericMapping against the round-1 files.
// Does not write either backup. Returns a report; throws when install must not proceed.
async function preflight({ allowPoc = false } = {}) {
  requireBackups();
  const liveManaged = await managed();
  const before = JSON.parse(readFileSync(BACKUP, "utf8"));
  const liveRepo = await ok("GET", "/openidm/config/repo.ds");
  const orphans = JSON.parse(readFileSync(REPO_ORPHANS, "utf8"));
  const repoBefore = JSON.parse(readFileSync(REPO_BEFORE, "utf8"));
  const managedSame = stable(liveManaged) === stable(before);
  const othersSame = stable(others(liveManaged)) === stable(others(before));
  const pocPresent = liveManaged.objects.filter((o) => POC_TYPES.includes(o.name)).map((o) => o.name);
  const repoMatchesOrphans = stable(gm(liveRepo)) === stable(gm(orphans));
  const repoMatchesBefore = stable(gm(liveRepo)) === stable(gm(repoBefore));
  const liveKeys = Object.keys(gm(liveRepo)).sort();
  const orphanKeys = Object.keys(gm(orphans)).sort();
  const extra = liveKeys.filter((k) => !orphanKeys.includes(k));
  const missing = orphanKeys.filter((k) => !liveKeys.includes(k));
  const report = {
    managedEqualsBackup: managedSame,
    nonPocEqualsBackup: othersSame,
    pocPresent,
    genericMappingEqualsRun1After: repoMatchesOrphans,
    genericMappingEqualsRun1Before: repoMatchesBefore,
    extraGenericMappingKeys: extra,
    missingGenericMappingKeys: missing,
  };
  console.log(JSON.stringify(report, null, 2));
  if (!repoMatchesOrphans) {
    throw new Error("live genericMapping does not equal .state/run1/repo.ds-after.json; refusing");
  }
  if (!othersSame) {
    throw new Error("a non-POC managed object differs from the backup; refusing");
  }
  if (!allowPoc && pocPresent.length) {
    throw new Error(`POC types already present (${pocPresent.join(",")}); refusing a clean install`);
  }
  return report;
}

// Apply `mutate` to our objects only, refusing if any other object differs
// from the backup (someone else is writing config/managed), then confirm the
// write by re-reading — config/managed is not read-your-writes (Q14).
async function rmw(mutate, label) {
  const before = JSON.parse(readFileSync(BACKUP, "utf8"));
  for (let attempt = 1; attempt <= 6; attempt++) {
    const doc = await managed();
    if (stable(others(doc)) !== stable(others(before))) {
      writeFileSync(join(STATE, `drift-${Date.now()}.json`), JSON.stringify(doc, null, 2));
      throw new Error(`${label}: a non-POC object differs from the backup; refusing to write`);
    }
    for (const o of doc.objects) if (POC_TYPES.includes(o.name)) mutate(o);
    const want = doc.objects.filter((o) => POC_TYPES.includes(o.name));
    const put = await call("PUT", "/openidm/config/managed", { body: doc });
    if (put.status !== 200) throw new Error(`${label}: PUT ${put.status} ${JSON.stringify(put.json)?.slice(0, 400)}`);
    for (let i = 0; i < 5; i++) {
      await sleep(1500);
      const now = (await managed()).objects.filter((o) => POC_TYPES.includes(o.name));
      if (stable(now) === stable(want)) {
        console.log(`${label}: confirmed (attempt ${attempt})`);
        return;
      }
    }
    console.log(`${label}: not confirmed, retrying`);
  }
  throw new Error(`${label}: write never confirmed`);
}

function ensureOrder(o, key) {
  if (!o.schema.order.includes(key)) o.schema.order.push(key);
}

function cloneMany(source, { title, reverseKey, target, keepStatus }) {
  const g = JSON.parse(JSON.stringify(source));
  g.title = title;
  const node = g.items;
  node.reversePropertyName = reverseKey;
  if (node.resourceCollection?.[0]) {
    node.resourceCollection[0].path = target;
    delete node.resourceCollection[0].notify;
    delete node.resourceCollection[0].notifySelf;
  }
  delete node.notify;
  delete node.notifySelf;
  delete node.notifyRelationships;
  if (!keepStatus && node.properties?._refProperties?.properties) {
    delete node.properties._refProperties.properties.status;
  }
  return g;
}

function applySchema(o) {
  const props = o.schema.properties;
  for (const [k, v] of Object.entries(FIELDS[o.name])) {
    props[k] = { title: k, ...v };
    ensureOrder(o, k);
  }
  if (o.name === PARENT) {
    const items = props.children.items;
    items.properties = items.properties || {};
    items.properties._refProperties = {
      type: "object",
      properties: { _id: { type: "string", propName: "_id", required: false }, status: { type: "string", title: "status" } },
    };
    // `relationship set` maps every pair it creates into repo.ds and has no
    // --generic, so every pair except children/parent is declared here: a
    // config/managed write never touches repo.ds.
    props.gchildren = cloneMany(props.children, {
      title: "gchildren", reverseKey: "gparent", target: `managed/${CHILD}`, keepStatus: true,
    });
    ensureOrder(o, "gchildren");
    // status stays on the holding end only, as on children/gchildren, so an
    // edge patch is the same write. The reverse schema does not declare it.
    props.peers = cloneMany(props.children, {
      title: "peers", reverseKey: "peerOf", target: `managed/${CHILD}`, keepStatus: true,
    });
    ensureOrder(o, "peers");
    props.owned = cloneMany(props.children, {
      title: "owned", reverseKey: "owner", target: `managed/${CHILD}`, keepStatus: true,
    });
    ensureOrder(o, "owned");
    o.onUpdate = hook("onUpdate.js");
    o.postUpdate = hook("postUpdate.js");
  }
  if (o.name === CHILD) {
    if (props.parent?.type !== "relationship") {
      throw new Error("alpha_poc_child.parent is not has-one; refusing to invent the generic pairs");
    }
    // Generic only. None of these go through relationship set, so repo.ds
    // gains no mapping for them.
    props.gparent = hasOneFrom(props.parent, {
      title: "gparent", reverseKey: "gchildren", target: `managed/${PARENT}`,
    });
    ensureOrder(o, "gparent");
    props.peerOf = hasManyFromOne(props.gparent, {
      title: "peerOf", reverseKey: "peers", target: `managed/${PARENT}`,
    });
    ensureOrder(o, "peerOf");
    props.owner = hasOneFrom(props.gparent, {
      title: "owner", reverseKey: "owned", target: `managed/${PARENT}`,
    });
    ensureOrder(o, "owner");
    o.onUpdate = hook("childOnUpdate.js");
    o.postUpdate = hook("childPostUpdate.js");
    o.onDelete = hook("childOnDelete.js");
  }
  setNotify(o);
  applyFanout(o);
  applyReturnByDefault(o);
}

function wrapMany(one, reverseKey) {
  const node = JSON.parse(JSON.stringify(one));
  delete node.title;
  delete node.description;
  delete node.searchable;
  delete node.viewable;
  delete node.userEditable;
  delete node.returnByDefault;
  node.reversePropertyName = reverseKey;
  return { type: "array", items: node };
}

function hasManyFromOne(one, { title, reverseKey, target }) {
  const wrapped = wrapMany(one, reverseKey);
  wrapped.title = title;
  wrapped.searchable = false;
  wrapped.viewable = true;
  wrapped.userEditable = true;
  wrapped.returnByDefault = false;
  wrapped.items.reversePropertyName = reverseKey;
  if (wrapped.items.resourceCollection?.[0]) {
    wrapped.items.resourceCollection[0].path = target;
    delete wrapped.items.resourceCollection[0].notify;
    delete wrapped.items.resourceCollection[0].notifySelf;
  }
  delete wrapped.items.notify;
  delete wrapped.items.notifySelf;
  if (wrapped.items.properties?._refProperties?.properties) {
    delete wrapped.items.properties._refProperties.properties.status;
  }
  return wrapped;
}

function hasOneFrom(one, { title, reverseKey, target }) {
  const node = JSON.parse(JSON.stringify(one));
  for (const k of ["title", "description", "searchable", "viewable", "userEditable", "returnByDefault", "notify", "notifySelf", "notifyRelationships"]) {
    delete node[k];
  }
  node.reversePropertyName = reverseKey;
  node.type = "relationship";
  if (node.resourceCollection?.[0]) {
    node.resourceCollection[0].path = target;
    delete node.resourceCollection[0].notify;
    delete node.resourceCollection[0].notifySelf;
  }
  if (node.properties?._refProperties?.properties) delete node.properties._refProperties.properties.status;
  return {
    title,
    description: "",
    searchable: false,
    viewable: true,
    userEditable: true,
    returnByDefault: false,
    ...node,
    type: "relationship",
  };
}

function nodeOf(prop) {
  if (!prop) return null;
  return prop.type === "array" ? prop.items : prop;
}

function clearFlags(prop) {
  const node = nodeOf(prop);
  if (!node) return;
  delete node.notify;
  delete node.notifySelf;
  if (Array.isArray(node.resourceCollection)) {
    for (const entry of node.resourceCollection) {
      delete entry.notify;
      delete entry.notifySelf;
    }
  }
}

// notifySelf lives on the relationship node on the tenant's own types.
// notify lives on that node in round 1, and on resourceCollection[0] on the
// tenant (FLAG_AT=collection). A per-relationship `at` in NOTIFY_SPEC wins.
function putFlag(prop, key, at) {
  const node = nodeOf(prop);
  if (!node) return;
  if (key === "notifySelf" || at !== "collection") {
    node[key] = true;
    return;
  }
  const entry = node.resourceCollection && node.resourceCollection[0];
  if (entry) entry.notify = true;
  else node.notify = true;
}

// NOTIFY=1: notifySelf on the parent object's relationships, notify on the
// child's. NOTIFY=2 swaps the ends. NOTIFY_SPEC is a JSON object keyed by
// relationship name, each { notify, notifySelf, at }, and replaces NOTIFY.
function setNotify(o) {
  const spec = process.env.NOTIFY_SPEC ? JSON.parse(process.env.NOTIFY_SPEC) : null;
  const atDefault = process.env.FLAG_AT || "node";
  const mode = process.env.NOTIFY || "";
  const rels = o.name === PARENT ? PARENT_RELS : o.name === CHILD ? CHILD_RELS : [];
  const end = o.name === PARENT ? "parent" : "child";
  for (const name of rels) {
    const prop = o.schema.properties[name];
    if (!prop) continue;
    clearFlags(prop);
    let wantNotify = false;
    let wantSelf = false;
    let at = atDefault;
    if (spec) {
      const s = spec[name] || {};
      wantNotify = !!s.notify;
      wantSelf = !!s.notifySelf;
      if (s.at) at = s.at;
    } else if (mode === "1" || mode === "2") {
      wantSelf = (mode === "1" && end === "parent") || (mode === "2" && end === "child");
      wantNotify = (mode === "1" && end === "child") || (mode === "2" && end === "parent");
    }
    if (wantSelf) putFlag(prop, "notifySelf", at);
    if (wantNotify) putFlag(prop, "notify", at);
  }
}

// Where a changed property names the relationships whose linked objects
// should be told. The tenant puts this on the property, not on the object.
// FANOUT=object-child / object-parent is the brief's object-level placement,
// measured because the tenant does not use it.
function clearFanout(o) {
  delete o.notifyRelationships;
  if (o.schema) delete o.schema.notifyRelationships;
  for (const prop of Object.values(o.schema?.properties || {})) {
    if (!prop || typeof prop !== "object") continue;
    delete prop.notifyRelationships;
    if (prop.items) delete prop.items.notifyRelationships;
  }
}

function applyFanout(o) {
  const mode = process.env.FANOUT || "off";
  clearFanout(o);
  if (mode === "off") return;
  if (mode === "object-child" && o.name === CHILD) o.notifyRelationships = ["parent", "gparent"];
  if (mode === "object-parent" && o.name === PARENT) o.notifyRelationships = ["children", "gchildren"];
  if (mode === "field-child" && o.name === CHILD) {
    o.schema.properties.value.notifyRelationships = ["parent", "gparent"];
  }
  if (mode === "field-parent" && o.name === PARENT) {
    o.schema.properties.name.notifyRelationships = ["children", "gchildren"];
  }
  if (mode === "rel-child" && o.name === CHILD) {
    o.schema.properties.parent.notifyRelationships = ["parent"];
    o.schema.properties.gparent.notifyRelationships = ["gparent"];
  }
  if (mode === "rel-parent" && o.name === PARENT) {
    o.schema.properties.children.notifyRelationships = ["children"];
    o.schema.properties.gchildren.notifyRelationships = ["gchildren"];
  }
  // Names the other relationship, the way alpha_organization.parent names
  // children. Naming a relationship from itself (rel-child / rel-parent)
  // took that managed resource offline: config still listed it, GET 404.
  if (mode === "rel-other-child" && o.name === CHILD) {
    o.schema.properties.parent.notifyRelationships = ["gparent"];
    o.schema.properties.gparent.notifyRelationships = ["parent"];
  }
  if (mode === "rel-other-parent" && o.name === PARENT) {
    o.schema.properties.children.notifyRelationships = ["gchildren"];
    o.schema.properties.gchildren.notifyRelationships = ["children"];
  }
  if (mode === "field-owner" && o.name === CHILD && o.schema.properties.owner) {
    o.schema.properties.value.notifyRelationships = ["owner"];
  }
  if (mode === "field-owned" && o.name === PARENT && o.schema.properties.owned) {
    o.schema.properties.name.notifyRelationships = ["owned"];
  }
}

function applyReturnByDefault(o) {
  const raw = process.env.RETURN_BY_DEFAULT;
  if (raw == null || raw === "") return;
  const want = raw === "1" || raw === "true";
  const rels = o.name === PARENT ? PARENT_RELS : o.name === CHILD ? CHILD_RELS : [];
  for (const name of rels) {
    const prop = o.schema.properties[name];
    if (prop) prop.returnByDefault = want;
  }
}

async function waitLive() {
  // A fold-mode parent proves the hook runs: derived appears after one patch.
  for (let i = 0; i < 40; i++) {
    const p = await ok("POST", `/openidm/managed/${PARENT}?_action=create`, { body: { name: "liveness", mode: "fold", inbox: [], entries: [] } });
    const r = await call("PATCH", `/openidm/managed/${PARENT}/${p._id}`, {
      body: [{ operation: "add", field: "/inbox/-", value: { id: "x", value: 1 } }],
    });
    await call("DELETE", `/openidm/managed/${PARENT}/${p._id}`);
    if (r.status === 200 && r.json?.derived?.count === 1 && r.json.inbox.length === 0) {
      console.log(`hooks live after ${i + 1} probe(s)`);
      return;
    }
    await sleep(3000);
  }
  throw new Error("hooks never went live");
}

// Names of POC types present on three consecutive reads. One read is not
// enough: config/managed is not read-your-writes (Q14), and a create that
// `aic` confirmed was lost to the next create on 2026-11-05.
async function stablyPresent() {
  let seen = null;
  for (let i = 0; i < 3; i++) {
    if (i) await sleep(3000);
    const now = (await managed()).objects.map((o) => o.name).filter((n) => POC_TYPES.includes(n));
    seen = seen ? seen.filter((n) => now.includes(n)) : now;
  }
  return seen;
}

async function install() {
  requireBackups();
  const doc = await managed();
  const presentNow = doc.objects.filter((o) => POC_TYPES.includes(o.name)).map((o) => o.name);
  await preflight({ allowPoc: presentNow.length > 0 });
  let present = await stablyPresent();
  for (let round = 1; ; round++) {
    const missing = POC_TYPES.filter((t) => !present.includes(t));
    if (!missing.length) break;
    if (round > 4) throw new Error(`types still missing: ${missing}`);
    for (const t of missing) {
      aic("managed", "object", "create", t, "--title", `POC ${t}`);
      await sleep(5000);
    }
    present = await stablyPresent();
    console.log(`round ${round}: present ${present.join(",")}`);
  }
  // The leftover genericMapping for children/parent is the round-1 orphan.
  // map_created_pair keeps a matching mapping and does not write repo.ds.
  // A second or different mapping is a failure: stop, do not PUT repo.ds.
  aic("managed", "relationship", "set", `${PARENT}.children`, "--target", CHILD, "--forward", "many", "--reverse", "one", "--reverse-key", "parent");
  const afterSet = await ok("GET", "/openidm/config/repo.ds");
  const orphans = JSON.parse(readFileSync(REPO_ORPHANS, "utf8"));
  if (stable(gm(afterSet)) !== stable(gm(orphans))) {
    writeFileSync(join(STATE, "repo.ds-after-set.json"), JSON.stringify(afterSet, null, 2));
    throw new Error("relationship set changed genericMapping; refusing to continue or to write repo.ds");
  }
  console.log("relationship set left genericMapping equal to run1/repo.ds-after.json");
  await rmw(applySchema, "schema+hooks");
  await waitLive();
}

async function hooks() {
  await rmw((o) => {
    if (o.name === PARENT) {
      o.onUpdate = hook("onUpdate.js");
      o.postUpdate = hook("postUpdate.js");
    }
    if (o.name === CHILD) {
      o.onUpdate = hook("childOnUpdate.js");
      o.postUpdate = hook("childPostUpdate.js");
      o.onDelete = hook("childOnDelete.js");
    }
  }, "hooks");
  await waitLive();
}

async function configure() {
  await rmw((o) => {
    setNotify(o);
    applyFanout(o);
    applyReturnByDefault(o);
  }, `configure notify=${process.env.NOTIFY || "off"} fanout=${process.env.FANOUT || "off"} at=${process.env.FLAG_AT || "node"}`);
}

// Child scripts off, flags left as they are. Used once, to see whether the
// child's own onDelete is what stops a delete from notifying the parent.
async function stripChildHooks() {
  await rmw((o) => {
    if (o.name !== CHILD) return;
    delete o.onUpdate;
    delete o.postUpdate;
    delete o.onDelete;
  }, "strip child hooks");
}

async function teardown() {
  requireBackups();
  for (const t of POC_TYPES) console.log(`${t}: deleted ${await deleteAll(t).catch(() => 0)} record(s)`);
  // The mapped pair first, so its repo.ds entries go with it. Round 1 showed
  // this refuses and writes nothing. That refusal is still the expected path.
  try {
    aic("managed", "relationship", "delete", `${PARENT}.children`, "--remove-mapping");
  } catch (e) {
    console.log(`children: delete failed: ${e.message}`);
  }
  for (const t of [PARENT, CHILD, SINK]) {
    try {
      aic("managed", "object", "delete", t);
    } catch (e) {
      console.log(`${t}: delete failed: ${e.message}`);
    }
  }
  for (const name of ["pocagg", "poctrace"]) {
    const deleted = await call("DELETE", `/openidm/config/endpoint/${name}`);
    const gone = deleted.status === 200 || deleted.status === 204 || deleted.status === 404;
    console.log(`endpoint ${name}: ${deleted.status}${gone ? "" : " " + JSON.stringify(deleted.json)?.slice(0, 200)}`);
    if (!gone) process.exitCode = 1;
  }
  const after = await managed();
  const before = JSON.parse(readFileSync(BACKUP, "utf8"));
  writeFileSync(join(STATE, "managed-after.json"), JSON.stringify(after, null, 2));
  const leftover = after.objects.filter((o) => POC_TYPES.includes(o.name)).map((o) => o.name);
  const same = stable(after.objects) === stable(before.objects);
  console.log(`leftover POC types: ${leftover.join(",") || "none"}; config equals backup: ${same}`);
  const repoAfter = await ok("GET", "/openidm/config/repo.ds");
  writeFileSync(join(STATE, "repo.ds-after.json"), JSON.stringify(repoAfter, null, 2));
  const orphans = JSON.parse(readFileSync(REPO_ORPHANS, "utf8"));
  const pocKeys = Object.keys(gm(repoAfter)).filter((k) => /alpha_poc_/.test(k));
  const repoSame = stable(gm(repoAfter)) === stable(gm(orphans));
  console.log(`repo.ds POC mappings left: ${pocKeys.join(",") || "none"}; genericMapping equals run1/repo.ds-after.json: ${repoSame}`);
  if (!repoSame) {
    const liveKeys = new Set(Object.keys(gm(repoAfter)));
    const wantKeys = new Set(Object.keys(gm(orphans)));
    const extra = [...liveKeys].filter((k) => !wantKeys.has(k));
    const missing = [...wantKeys].filter((k) => !liveKeys.has(k));
    const changed = [...liveKeys].filter((k) => wantKeys.has(k) && stable(gm(repoAfter)[k]) !== stable(gm(orphans)[k]));
    console.log(`genericMapping extra: ${extra.join(",") || "none"}; missing: ${missing.join(",") || "none"}; changed: ${changed.join(",") || "none"}`);
  }
  if (!same || !repoSame) process.exitCode = 1;
}

const cmd = process.argv[2];
const isMain = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isMain) {
  await ({
    preflight: () => preflight({ allowPoc: process.env.ALLOW_POC === "1" }),
    install,
    hooks,
    schema: () => rmw(applySchema, "schema+hooks").then(waitLive),
    configure,
    notify: configure,
    "strip-child-hooks": stripChildHooks,
    teardown,
    live: waitLive,
  }[cmd] || (() => console.error("usage: preflight|install|hooks|schema|configure|teardown|live")))();
}
