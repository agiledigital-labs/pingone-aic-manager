// Install / remove the POC's throwaway managed types.
//
//   node setup.mjs install    backup, create types, relationship, schema+hooks
//   node setup.mjs hooks      re-push the hook sources only
//   node setup.mjs teardown   delete records and types, compare with backup
//
// The backup and every config/managed snapshot go to .state/ (gitignored):
// they hold the whole tenant schema.

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { ok, call, sleep, PROJECT, PARENT, CHILD, SINK, POC_TYPES, deleteAll } from "./lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const STATE = join(HERE, ".state");
mkdirSync(STATE, { recursive: true });
const BACKUP = join(STATE, "managed-before.json");

const aic = (...args) =>
  execFileSync(process.env.AIC || "aic", ["--no-prompt", ...args], {
    encoding: "utf8",
    env: { ...process.env, AIC_PROJECT: PROJECT },
    stdio: ["ignore", "pipe", "inherit"],
  });

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
  [CHILD]: { label: str({ searchable: true }), value: num(), rank: num() },
  [SINK]: {
    kind: str({ searchable: true }),
    parent: str({ searchable: true }),
    runId: str(),
    rev: str(),
    oldRev: str(),
    outcome: str(),
    count: num(),
    inboxLen: num(),
    oldCount: num(),
    ops: str(),
    note: str(),
    at: num(),
  },
};

const hook = (file) => ({ type: "text/javascript", source: readFileSync(join(HERE, "hooks", file), "utf8") });

const managed = () => ok("GET", "/openidm/config/managed");
const others = (doc) => doc.objects.filter((o) => !POC_TYPES.includes(o.name));
const stable = (v) => JSON.stringify(v);

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
    if (put.status !== 200) throw new Error(`${label}: PUT ${put.status} ${JSON.stringify(put.json)}`);
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

function applySchema(o) {
  const props = o.schema.properties;
  for (const [k, v] of Object.entries(FIELDS[o.name])) {
    props[k] = { title: k, ...v };
    if (!o.schema.order.includes(k)) o.schema.order.push(k);
  }
  if (o.name === PARENT) {
    const items = props.children.items;
    items.properties = items.properties || {};
    items.properties._refProperties = {
      type: "object",
      properties: { _id: { type: "string", propName: "_id", required: false }, status: { type: "string", title: "status" } },
    };
    // `relationship set` maps every pair it creates into repo.ds and has no
    // --generic, so the generic pair is declared here: a config/managed write
    // never touches repo.ds.
    const g = JSON.parse(JSON.stringify(props.children));
    g.title = "gchildren";
    g.items.reversePropertyName = "gparent";
    props.gchildren = g;
    if (!o.schema.order.includes("gchildren")) o.schema.order.push("gchildren");
    o.onUpdate = hook("onUpdate.js");
    o.postUpdate = hook("postUpdate.js");
  }
  if (o.name === CHILD) {
    const g = JSON.parse(JSON.stringify(props.parent));
    g.title = "gparent";
    g.reversePropertyName = "gchildren";
    props.gparent = g;
    if (!o.schema.order.includes("gparent")) o.schema.order.push("gparent");
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
  const doc = await managed();
  if (!existsSync(BACKUP)) {
    if (doc.objects.some((o) => POC_TYPES.includes(o.name))) {
      throw new Error("a POC type exists but there is no backup; refusing");
    }
    writeFileSync(BACKUP, JSON.stringify(doc, null, 2));
  }
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
  aic("managed", "relationship", "set", `${PARENT}.children`, "--target", CHILD, "--forward", "many", "--reverse", "one", "--reverse-key", "parent");
  await rmw(applySchema, "schema+hooks");
  await waitLive();
}

async function hooks() {
  await rmw((o) => {
    if (o.name === PARENT) {
      o.onUpdate = hook("onUpdate.js");
      o.postUpdate = hook("postUpdate.js");
    }
  }, "hooks");
  await waitLive();
}

async function teardown() {
  for (const t of POC_TYPES) console.log(`${t}: deleted ${await deleteAll(t).catch(() => 0)} record(s)`);
  // The mapped pair first, so its repo.ds entries go with it.
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
  const after = await managed();
  const before = JSON.parse(readFileSync(BACKUP, "utf8"));
  writeFileSync(join(STATE, "managed-after.json"), JSON.stringify(after, null, 2));
  const leftover = after.objects.filter((o) => POC_TYPES.includes(o.name)).map((o) => o.name);
  const same = stable(after.objects) === stable(before.objects);
  console.log(`leftover POC types: ${leftover.join(",") || "none"}; config equals backup: ${same}`);
  const repoAfter = await ok("GET", "/openidm/config/repo.ds");
  writeFileSync(join(STATE, "repo.ds-after.json"), JSON.stringify(repoAfter, null, 2));
  const repoBefore = JSON.parse(readFileSync(join(STATE, "repo.ds-before.json"), "utf8"));
  const gm = (d) => d.resourceMapping.genericMapping;
  const pocKeys = Object.keys(gm(repoAfter)).filter((k) => /alpha_poc_/.test(k));
  const repoSame = stable(gm(repoAfter)) === stable(gm(repoBefore));
  console.log(`repo.ds POC mappings left: ${pocKeys.join(",") || "none"}; genericMapping equals backup: ${repoSame}`);
  if (!same || !repoSame) process.exitCode = 1;
}

const cmd = process.argv[2];
await ({ install, hooks, schema: () => rmw(applySchema, "schema+hooks").then(waitLive), teardown, live: waitLive }[cmd] || (() => console.error("usage: install|hooks|teardown|live")))();
