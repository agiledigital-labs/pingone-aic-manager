/**
 * E15: IDM's own runtime scripts come from a project-supplied provider. Every
 * script here is an invented stand-in (test/fixtures/idm-runtime-scripts, or
 * written below), run on the real JVM through the emulator's own hook and
 * endpoint paths.
 */
import { chmodSync, mkdirSync, mkdtempSync, renameSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { currentEmulationOwner, emulationAudit } from "../../src/emulation.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { tallyFaults } from "../../src/harness/fault-tally.ts";
import { IdmConfig, IdmEmulator, type ManagedType, type RuntimeScriptProvider } from "../../src/idm/index.ts";
import type { RestResult } from "../../src/idm/types.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js, send } from "./fixtures.ts";

const fixtureUrl = new URL("../fixtures/idm-runtime-scripts/", import.meta.url);
const directory = fileURLToPath(fixtureUrl);

let runner: RhinoRunner;
/** A scratch tree this file creates and removes: a provider root plus a file outside it. */
let scratch: string;
let linkedRoot: string;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
  scratch = mkdtempSync(join(tmpdir(), "rhino-local-e15-"));
  linkedRoot = join(scratch, "root");
  const outside = join(scratch, "outside");
  mkdirSync(join(linkedRoot, "lib"), { recursive: true });
  mkdirSync(outside);
  const stamp = "object.stamp = 'from-outside-the-root';";
  writeFileSync(join(outside, "stamp.js"), stamp);
  writeFileSync(join(linkedRoot, "lib", "ok.js"), "object.stamp = 'ok';");
  symlinkSync(join(outside, "stamp.js"), join(linkedRoot, "lib", "stamp.js"));
  symlinkSync(outside, join(linkedRoot, "dirlink"));
}, 60_000);
afterAll(async () => {
  await runner?.close();
  if (scratch) rmSync(scratch, { recursive: true, force: true });
}, 30_000);

const widget = (hooks: Partial<ManagedType> = {}): ManagedType => ({
  name: "widget", schema: { properties: { text: { type: "string" }, stamp: { type: "string" }, label: { type: "string" } } }, ...hooks,
});
const fileHook = (file: string): Partial<ManagedType> => ({ onCreate: { type: "text/javascript", file } });

function emulator(options: { runtimeScripts?: RuntimeScriptProvider; type?: ManagedType; endpoint?: string } = {}): IdmEmulator {
  return new IdmEmulator(new IdmConfig({ sync: { mappings: [] },
    managed: { objects: [options.type ?? widget()] },
    ...(options.endpoint === undefined ? {} : { endpoints: { probe: js(options.endpoint) } }),
  }), runner, options.runtimeScripts === undefined ? {} : { runtimeScripts: options.runtimeScripts });
}

const createWidget = (emu: IdmEmulator, status = 201) =>
  send(emu, { method: "PUT", path: "managed/widget/w1", headers: { "If-None-Match": "*" }, body: { text: "hello" } }, status);
const tryCreate = (emu: IdmEmulator) =>
  emu.request({ method: "PUT", path: "managed/widget/w1", headers: { "If-None-Match": "*" }, body: { text: "hello" } });
const probe = (emu: IdmEmulator) => emu.request({ method: "GET", path: "endpoint/probe" });

/** A loader over an in-memory map that counts what it was asked for. */
function loader(scripts: Record<string, string>): { provider: RuntimeScriptProvider; asked: string[] } {
  const asked: string[] = [];
  return { asked, provider: (path) => { asked.push(path); return scripts[path]; } };
}

/** The ids of a failed result's harness faults, and its fault message text. */
const faultIds = (result: RestResult) => result.emulation?.faults?.map((fault) => fault.id);
const faultText = (result: RestResult) => result.emulation?.faults?.map((fault) => fault.message).join("\n") ?? "";

/** This case's own row for `id` in the fault tally, as the reporter computes it. */
function tallyRow(id: string) {
  const audit = emulationAudit();
  const mine = { ...audit, failures: audit.failures.filter((entry) => entry.owner?.id === currentEmulationOwner()?.id) };
  return tallyFaults([mine]).faults.find((fault) => fault.id === id);
}

describe("supplied runtime scripts", () => {
  it("a hook's file reference runs the directory provider's script with the hook's bindings", async () => {
    const emu = emulator({ runtimeScripts: directory, type: widget(fileHook("standin/stamp.js")) });
    const body = await createWidget(emu);
    expect(body.stamp).toBe("stamped:create:managed/widget/w1");
    // Forks share the provider.
    const fork = emu.fork();
    const again = await send(fork, { method: "PUT", path: "managed/widget/w2", headers: { "If-None-Match": "*" }, body: { text: "x" } }, 201);
    expect(again.stamp).toBe("stamped:create:managed/widget/w2");
  });

  it("a file: URL names the directory as well as an absolute path does", async () => {
    const emu = emulator({ runtimeScripts: fixtureUrl, type: widget(fileHook("standin/stamp.js")) });
    expect((await createWidget(emu)).stamp).toBe("stamped:create:managed/widget/w1");
  });

  it("require loads a module that sees the caller's bindings and keeps its own vars", async () => {
    const emu = emulator({ runtimeScripts: directory, type: widget({ onCreate: js(
      "var m = require('standin/label');" +
      "object.label = m.label('L') + '|' + (typeof hidden) + '|' + (m === require('standin/label'));") }) });
    const body = await createWidget(emu);
    expect(body.label).toBe("L:hello|undefined|true");
  });

  it("a module body runs once per id string per invocation: 'x' and 'x.js' are two instances", async () => {
    // Module instances are assumed to be keyed by the id as written (unmeasured on AIC), so a module
    // with an effect runs once for each spelling. Merging the spellings would run it once here
    // and twice on AIC.
    const counter = loader({ "standin/count.js": "effects.push(module.id); exports.calls = 1;" });
    const emu = emulator({ runtimeScripts: counter.provider, endpoint:
      "var effects = []; var a = require('standin/count'); a.calls++; var b = require('standin/count'); var c = require('standin/count.js');" +
      "({ same: a === b, spellings: a === c, calls: b.calls, other: c.calls, effects: effects });" });
    const expected = { same: true, spellings: false, calls: 2, other: 1, effects: ["standin/count", "standin/count.js"] };
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject(expected);
    // The next invocation runs both again: the cache is per invocation, not per engine.
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject(expected);
    expect(counter.asked).toEqual(Array(4).fill("standin/count.js"));
  });

  it("a module that throws is not cached, so the next require runs it again", async () => {
    const boom = loader({ "standin/boom.js": "effects.push(1); throw new Error('boom');" });
    const emu = emulator({ runtimeScripts: boom.provider, endpoint:
      "var effects = []; var seen = [];" +
      "try { require('standin/boom'); } catch (e) { seen.push(String(e.message)); }" +
      "try { require('standin/boom'); seen.push('cached'); } catch (e) { seen.push(String(e.message)); }" +
      "({ runs: effects.length, seen: seen });" });
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject({ runs: 2, seen: ["boom", "boom"] });
  });

  it("a require cycle sees the partial exports", async () => {
    const cyclic = loader({
      "standin/a.js": "exports.early = 'a-early'; var b = require('standin/b'); exports.late = 'a-late'; exports.fromB = b.sawA;",
      "standin/b.js": "var a = require('standin/a'); exports.sawA = String(a.early) + '/' + String(a.late);",
    });
    const emu = emulator({ runtimeScripts: cyclic.provider, endpoint: "({ fromB: require('standin/a').fromB });" });
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject({ fromB: "a-early/undefined" });
  });

  it("a cycle after module.exports = … sees the original exports object, not the replacement", async () => {
    const cyclic = loader({
      "standin/a.js": "module.exports = { f: function () { return 'f'; } }; var b = require('standin/b'); module.exports.fromB = b.sawA;",
      "standin/b.js": "var a = require('standin/a'); exports.sawA = typeof a.f;",
    });
    const emu = emulator({ runtimeScripts: cyclic.provider, endpoint:
      "var a = require('standin/a'); ({ fromB: a.fromB, f: a.f(), again: a === require('standin/a') });" });
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject({ fromB: "undefined", f: "f", again: true });
  });

  it("a primitive module.exports comes back as an object", async () => {
    const primitive = loader({ "standin/p.js": "module.exports = 'text';" });
    const emu = emulator({ runtimeScripts: primitive.provider, endpoint:
      "var p = require('standin/p'); ({ type: typeof p, text: String(p) });" });
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject({ type: "object", text: "text" });
  });
});

describe("unsupplied runtime scripts fail as missing", () => {
  it("a file reference with no provider names the script and the fetch, and says the fetch is planned", async () => {
    expectHarnessFault("idm-runtime-script/roles/standin-absent.js");
    const result = await tryCreate(emulator({ type: widget(fileHook("roles/standin-absent.js")) }));
    expect(result.status).toBe(501);
    expect(faultIds(result)).toEqual(["idm-runtime-script/roles/standin-absent.js"]);
    const message = faultText(result);
    expect(message).toContain("roles/standin-absent.js");
    expect(message).toContain("no runtime-script provider is configured");
    expect(message).toContain("`aic idm runtime-scripts pull` is planned and not yet shipped");
  });

  it("a directory without the file says so", async () => {
    expectHarnessFault("idm-runtime-script/standin/absent.js");
    const result = await tryCreate(emulator({ runtimeScripts: directory, type: widget(fileHook("standin/absent.js")) }));
    expect(result.status).toBe(501);
    expect(faultText(result)).toContain("has no such file");
  });

  it("a require the script catches still fails the request, and reaches the tally", async () => {
    expectHarnessFault("idm-runtime-script/standin/absent.js");
    const empty = loader({});
    const emu = emulator({ runtimeScripts: empty.provider,
      endpoint: "var r; try { require('standin/absent'); r = 'loaded'; } catch (e) { r = 'caught'; } ({ r: r });" });
    const result = await probe(emu);
    expect(result.status, JSON.stringify(result.body)).toBe(501);
    expect(result.emulation?.kind).toBe("missing");
    expect(empty.asked).toEqual(["standin/absent.js"]);
    const row = tallyRow("idm-runtime-script/standin/absent.js");
    expect(row).toMatchObject({ cases: 1 });
    expect(row?.files.some((file) => file.endsWith("runtime-scripts.test.ts"))).toBe(true);
    expect(row?.message).toContain("require(\"standin/absent\")");
  });

  it("a caught openidm write whose hook names an unsupplied file still fails, and keeps its faults on the outer result", async () => {
    expectHarnessFault("idm-runtime-script/standin/absent.js");
    const emu = emulator({ runtimeScripts: directory, type: widget(fileHook("standin/absent.js")),
      endpoint: "var r; try { openidm.create('managed/widget', 'w9', { text: 'x' }); r = 'created'; } catch (e) { r = 'caught'; } ({ r: r });" });
    const result = await probe(emu);
    expect(result.status).toBe(501);
    expect(faultIds(result)).toEqual(["idm-runtime-script/standin/absent.js"]);
    expect(tallyRow("idm-runtime-script/standin/absent.js")).toMatchObject({ cases: 1 });
  });

  it("a nested require fault inside a caught write keeps its faults on the outer result", async () => {
    expectHarnessFault("idm-runtime-script/standin/absent.js");
    const emu = emulator({ runtimeScripts: loader({}).provider, type: widget({ onCreate: js("require('standin/absent');") }),
      endpoint: "var r; try { openidm.create('managed/widget', 'w9', { text: 'x' }); r = 'created'; } catch (e) { r = 'caught'; } ({ r: r });" });
    const result = await probe(emu);
    expect(result.status).toBe(501);
    expect(faultIds(result)).toEqual(["idm-runtime-script/standin/absent.js"]);
  });

  it("an id that leaves the script root is its own fault", async () => {
    expectHarnessFault("idm-runtime-script-path");
    const emu = emulator({ runtimeScripts: directory, endpoint: "try { require('../outside'); } catch (e) {} ({});" });
    expect((await probe(emu)).status).toBe(501);
  });

  it.each([
    ["./standin/stamp.js", "a \".\" segment"],
    ["standin/../standin/stamp.js", "a \"..\" segment"],
    ["standin//stamp.js", "an empty segment"],
    ["standin/stamp.js/", "ends with a slash"],
    ["standin/stamp.js\0", "a NUL byte"],
    ["/standin/stamp.js", "absolute"],
    ["standin\\stamp.js", "a backslash"],
  ])("a non-canonical file name %j is the path fault, and the provider is never asked", async (file, why) => {
    expectHarnessFault("idm-runtime-script-path");
    const asked = loader({ "standin/stamp.js": "object.stamp = 'loaded';" });
    const result = await tryCreate(emulator({ runtimeScripts: asked.provider, type: widget(fileHook(file)) }));
    expect(result.status).toBe(501);
    expect(faultIds(result)).toEqual(["idm-runtime-script-path"]);
    expect(faultText(result)).toContain(why);
    expect(asked.asked).toEqual([]);
  });

  it("a script declaring both source and file is refused with an id, not run", async () => {
    expectHarnessFault("idm-script-file-and-source");
    const result = await tryCreate(emulator({ runtimeScripts: directory,
      type: widget({ onCreate: { type: "text/javascript", file: "standin/stamp.js", source: "object.stamp = 'inline';" } }) }));
    expect(result.status).toBe(501);
    expect(faultIds(result)).toEqual(["idm-script-file-and-source"]);
  });
});

describe("the directory provider follows no links and matches names exactly", () => {
  it("a symlinked file inside the root is the path fault, and the outside script never runs", async () => {
    // The reviewer's end-to-end repro: lib/stamp.js -> <outside>/stamp.js used to run the
    // outside script and answer 201 with its stamp.
    expectHarnessFault("idm-runtime-script-path");
    const emu = emulator({ runtimeScripts: linkedRoot, type: widget(fileHook("lib/stamp.js")) });
    const result = await tryCreate(emu);
    expect(result.status).toBe(501);
    expect(faultIds(result)).toEqual(["idm-runtime-script-path"]);
    expect(faultText(result)).toContain("symbolic link");
    expect(JSON.stringify(result.body)).not.toContain("from-outside-the-root");
    expect((await emu.request({ method: "GET", path: "managed/widget/w1" })).status).toBe(404);
  });

  it("a path through a symlinked directory is the path fault", async () => {
    expectHarnessFault("idm-runtime-script-path");
    const result = await tryCreate(emulator({ runtimeScripts: linkedRoot, type: widget(fileHook("dirlink/stamp.js")) }));
    expect(faultIds(result)).toEqual(["idm-runtime-script-path"]);
    expect(faultText(result)).toContain("dirlink");
  });

  it("a regular file beside the links still loads", async () => {
    const body = await createWidget(emulator({ runtimeScripts: linkedRoot, type: widget(fileHook("lib/ok.js")) }));
    expect(body.stamp).toBe("ok");
  });

  it("a name that differs only in case is unsupplied, and the message names the near match", async () => {
    // AIC's lookup is case-sensitive; a case-insensitive filesystem would otherwise open it.
    expectHarnessFault("idm-runtime-script/standin/Stamp.js");
    const result = await tryCreate(emulator({ runtimeScripts: directory, type: widget(fileHook("standin/Stamp.js")) }));
    expect(faultIds(result)).toEqual(["idm-runtime-script/standin/Stamp.js"]);
    expect(faultText(result)).toContain("standin/stamp.js, which differs in case");
  });

  it("a directory name is unsupplied, not an I/O error", async () => {
    expectHarnessFault("idm-runtime-script/standin");
    const result = await tryCreate(emulator({ runtimeScripts: directory, type: widget(fileHook("standin")) }));
    expect(result.emulation?.kind).toBe("missing");
    expect(faultIds(result)).toEqual(["idm-runtime-script/standin"]);
    expect(faultText(result)).toContain("a directory, not a file");
  });

  it("the root is indexed once, at construction: a file added later is unsupplied", async () => {
    expectHarnessFault("idm-runtime-script/lib/late.js");
    const emu = emulator({ runtimeScripts: linkedRoot, type: widget(fileHook("lib/late.js")) });
    writeFileSync(join(linkedRoot, "lib", "late.js"), "object.stamp = 'late';");
    try {
      const result = await tryCreate(emu);
      expect(faultIds(result)).toEqual(["idm-runtime-script/lib/late.js"]);
      // The file exists when anyone looks, so the message must say which view was consulted.
      expect(faultText(result)).toContain("no such file as indexed when the emulator was constructed");
    } finally {
      rmSync(join(linkedRoot, "lib", "late.js"));
    }
  });
});

describe("a link swapped in after construction is refused at read time (review e15-02 P3-A)", () => {
  /** A fresh provider root with `<dir>/<name>` regular, plus an outside directory holding the same name. */
  function swapTree(dir: string, name: string): { root: string; outside: string } {
    const base = mkdtempSync(join(scratch, "swap-"));
    const root = join(base, "root");
    const outside = join(base, "outside");
    mkdirSync(join(root, dir), { recursive: true });
    mkdirSync(outside);
    writeFileSync(join(root, dir, name), "object.stamp = 'inside';");
    writeFileSync(join(outside, name), "object.stamp = 'OUTSIDE';");
    return { root, outside };
  }

  it("a file replaced by a link after construction is the path fault, and the outside script never runs", async () => {
    // The reviewer's repro gave 201 with stamp OUTSIDE: the index said "file", and the read followed the link.
    expectHarnessFault("idm-runtime-script-path");
    const { root, outside } = swapTree("lib", "late-swap.js");
    const emu = emulator({ runtimeScripts: root, type: widget(fileHook("lib/late-swap.js")) });
    unlinkSync(join(root, "lib", "late-swap.js"));
    symlinkSync(join(outside, "late-swap.js"), join(root, "lib", "late-swap.js"));
    const result = await tryCreate(emu);
    expect(result.status).toBe(501);
    expect(faultIds(result)).toEqual(["idm-runtime-script-path"]);
    expect(faultText(result)).toContain("replaced by a symbolic link");
    expect(JSON.stringify(result.body)).not.toContain("OUTSIDE");
  });

  it("a directory on the path replaced by a link after construction is the path fault", async () => {
    // O_NOFOLLOW guards only the last component; this case needs the realpath and identity check.
    expectHarnessFault("idm-runtime-script-path");
    const { root, outside } = swapTree("sub", "swap.js");
    const emu = emulator({ runtimeScripts: root, type: widget(fileHook("sub/swap.js")) });
    renameSync(join(root, "sub"), join(root, "sub-original"));
    symlinkSync(outside, join(root, "sub"));
    const result = await tryCreate(emu);
    expect(faultIds(result)).toEqual(["idm-runtime-script-path"]);
    expect(JSON.stringify(result.body)).not.toContain("OUTSIDE");
  });

  it("a root that is itself reached through a link still loads its regular files", async () => {
    // The containment check compares against the root's resolved path, not the spelling passed in.
    const { root } = swapTree("lib", "plain.js");
    const alias = join(root, "..", "alias");
    symlinkSync(root, alias);
    const body = await createWidget(emulator({ runtimeScripts: alias, type: widget(fileHook("lib/plain.js")) }));
    expect(body.stamp).toBe("inside");
  });
});

describe("a failing loader is an ided fault", () => {
  it("a loader that throws", async () => {
    expectHarnessFault("idm-runtime-script-provider");
    const result = await tryCreate(emulator({ runtimeScripts: () => { throw new Error("loader boom"); }, type: widget(fileHook("standin/stamp.js")) }));
    expect(result.status).toBe(501);
    expect(faultIds(result)).toEqual(["idm-runtime-script-provider"]);
    expect(faultText(result)).toContain("loader boom");
  });

  it("a loader that returns a non-string", async () => {
    expectHarnessFault("idm-runtime-script-provider");
    const result = await tryCreate(emulator({ runtimeScripts: () => 42 as unknown as string, type: widget(fileHook("standin/stamp.js")) }));
    expect(faultIds(result)).toEqual(["idm-runtime-script-provider"]);
    expect(faultText(result)).toContain("returned number");
  });
});

describe("provider configuration", () => {
  it("refuses a provider of the wrong shape", () => {
    expect(() => emulator({ runtimeScripts: 42 as unknown as string })).toThrow(/runtimeScripts/);
    expect(() => emulator({ runtimeScripts: "" })).toThrow(/runtimeScripts/);
  });

  it("refuses a relative directory, because its base would be the process cwd", () => {
    expect(() => emulator({ runtimeScripts: "idm-runtime-scripts" })).toThrow(/is relative; pass an absolute path or a file: URL/);
  });

  it("refuses a directory that does not exist or is a file, naming it", () => {
    const absent = join(directory, "no-such-directory");
    expect(() => emulator({ runtimeScripts: absent })).toThrow(new RegExp(`${absent.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} is not a directory`));
    expect(() => emulator({ runtimeScripts: join(directory, "manifest.json") })).toThrow(/is not a directory/);
    expect(() => emulator({ runtimeScripts: new URL("https://example.com/scripts/") })).toThrow(/not a file: URL/);
  });

  it.skipIf(process.getuid?.() === 0)("an unreadable subdirectory is the documented TypeError naming the root, not a raw EACCES", () => {
    const root = mkdtempSync(join(scratch, "unreadable-"));
    mkdirSync(join(root, "locked"));
    chmodSync(join(root, "locked"), 0o000);
    try {
      expect(() => emulator({ runtimeScripts: root })).toThrow(TypeError);
      expect(() => emulator({ runtimeScripts: root })).toThrow(`Invalid IDM runtimeScripts: cannot index ${root}`);
    } finally {
      chmodSync(join(root, "locked"), 0o755);
    }
  });
});
