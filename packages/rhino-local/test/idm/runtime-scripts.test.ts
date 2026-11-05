/**
 * E15: IDM's own runtime scripts come from a project-supplied provider. Every
 * script here is an invented stand-in (test/fixtures/idm-runtime-scripts), run
 * on the real JVM through the emulator's own hook and endpoint paths.
 */
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { currentEmulationOwner, emulationAudit } from "../../src/emulation.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { tallyFaults } from "../../src/harness/fault-tally.ts";
import { IdmConfig, IdmEmulator, type ManagedType, type RuntimeScriptProvider } from "../../src/idm/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js, send } from "./fixtures.ts";

const directory = fileURLToPath(new URL("../fixtures/idm-runtime-scripts", import.meta.url));

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

const widget = (hooks: Partial<ManagedType> = {}): ManagedType => ({
  name: "widget", schema: { properties: { text: { type: "string" }, stamp: { type: "string" }, label: { type: "string" } } }, ...hooks,
});

function emulator(options: { runtimeScripts?: RuntimeScriptProvider; type?: ManagedType; endpoint?: string } = {}): IdmEmulator {
  return new IdmEmulator(new IdmConfig({
    managed: { objects: [options.type ?? widget()] },
    ...(options.endpoint === undefined ? {} : { endpoints: { probe: js(options.endpoint) } }),
  }), runner, options.runtimeScripts === undefined ? {} : { runtimeScripts: options.runtimeScripts });
}

const createWidget = (emu: IdmEmulator, status = 201) =>
  send(emu, { method: "PUT", path: "managed/widget/w1", headers: { "If-None-Match": "*" }, body: { text: "hello" } }, status);

/** A loader over an in-memory map that counts what it was asked for. */
function loader(scripts: Record<string, string>): { provider: RuntimeScriptProvider; asked: string[] } {
  const asked: string[] = [];
  return { asked, provider: (path) => { asked.push(path); return scripts[path]; } };
}

describe("supplied runtime scripts", () => {
  it("a hook's file reference runs the directory provider's script with the hook's bindings", async () => {
    const emu = emulator({ runtimeScripts: directory, type: widget({ onCreate: { type: "text/javascript", file: "standin/stamp.js" } }) });
    const body = await createWidget(emu);
    expect(body.stamp).toBe("stamped:create:managed/widget/w1");
    // Forks share the provider.
    const fork = emu.fork();
    const again = await send(fork, { method: "PUT", path: "managed/widget/w2", headers: { "If-None-Match": "*" }, body: { text: "x" } }, 201);
    expect(again.stamp).toBe("stamped:create:managed/widget/w2");
  });

  it("require loads a module that sees the caller's bindings and keeps its own vars", async () => {
    const emu = emulator({ runtimeScripts: directory, type: widget({ onCreate: js(
      "var m = require('standin/label');" +
      "object.label = m.label('L') + '|' + (typeof hidden) + '|' + (m === require('standin/label.js'));") }) });
    const body = await createWidget(emu);
    expect(body.label).toBe("L:hello|undefined|true");
  });

  it("a module runs once per invocation, and again in the next one", async () => {
    const counter = loader({ "standin/count.js": "exports.n = (typeof seen === 'undefined' ? 0 : 1); var seen = true; exports.calls = 1;" });
    const emu = emulator({ runtimeScripts: counter.provider,
      endpoint: "var a = require('standin/count'); a.calls++; var b = require('standin/count'); ({ same: a === b, calls: b.calls, n: b.n });" });
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject({ same: true, calls: 2, n: 0 });
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject({ same: true, calls: 2, n: 0 });
    // Asked once per invocation: the cache is per invocation, not per engine.
    expect(counter.asked).toEqual(["standin/count.js", "standin/count.js"]);
  });

  it("a require cycle sees the partial exports", async () => {
    const cyclic = loader({
      "standin/a.js": "exports.early = 'a-early'; var b = require('standin/b'); exports.late = 'a-late'; exports.fromB = b.sawA;",
      "standin/b.js": "var a = require('standin/a'); exports.sawA = String(a.early) + '/' + String(a.late);",
    });
    const emu = emulator({ runtimeScripts: cyclic.provider, endpoint: "({ fromB: require('standin/a').fromB });" });
    expect(await send(emu, { method: "GET", path: "endpoint/probe" })).toMatchObject({ fromB: "a-early/undefined" });
  });
});

describe("unsupplied runtime scripts fail as missing", () => {
  it("a file reference with no provider names the script and the fetch", async () => {
    expectHarnessFault("idm-runtime-script/roles/standin-absent.js");
    const emu = emulator({ type: widget({ onCreate: { type: "text/javascript", file: "roles/standin-absent.js" } }) });
    const result = await emu.request({ method: "PUT", path: "managed/widget/w1", headers: { "If-None-Match": "*" }, body: { text: "x" } });
    expect(result.status).toBe(501);
    expect(result.emulation?.faults?.map((fault) => fault.id)).toEqual(["idm-runtime-script/roles/standin-absent.js"]);
    const message = result.emulation?.faults?.[0]?.message ?? "";
    expect(message).toContain("roles/standin-absent.js");
    expect(message).toContain("no runtime-script provider is configured");
    expect(message).toContain("aic idm runtime-scripts pull");
  });

  it("a directory without the file says so", async () => {
    expectHarnessFault("idm-runtime-script/standin/absent.js");
    const emu = emulator({ runtimeScripts: directory, type: widget({ onCreate: { type: "text/javascript", file: "standin/absent.js" } }) });
    const result = await createWidget(emu, 501);
    expect(JSON.stringify(result)).toContain("has no such file");
  });

  it("a require the script catches still fails the request, and reaches the tally", async () => {
    expectHarnessFault("idm-runtime-script/standin/absent.js");
    const empty = loader({});
    const emu = emulator({ runtimeScripts: empty.provider,
      endpoint: "var r; try { require('standin/absent'); r = 'loaded'; } catch (e) { r = 'caught'; } ({ r: r });" });
    const result = await emu.request({ method: "GET", path: "endpoint/probe" });
    expect(result.status, JSON.stringify(result.body)).toBe(501);
    expect(result.emulation?.kind).toBe("missing");
    expect(empty.asked).toEqual(["standin/absent.js"]);
    // This case's own audit entries, as the reporter tallies them.
    const audit = emulationAudit();
    const mine = { ...audit, failures: audit.failures.filter((entry) => entry.owner?.id === currentEmulationOwner()?.id) };
    const row = tallyFaults([mine]).faults.find((fault) => fault.id === "idm-runtime-script/standin/absent.js");
    expect(row).toMatchObject({ cases: 1 });
    expect(row?.files.some((file) => file.endsWith("runtime-scripts.test.ts"))).toBe(true);
    expect(row?.message).toContain("require(\"standin/absent\")");
  });

  it("a caught openidm write whose hook names an unsupplied file still fails", async () => {
    expectHarnessFault("idm-runtime-script/standin/absent.js");
    const emu = emulator({ runtimeScripts: directory, type: widget({ onCreate: { type: "text/javascript", file: "standin/absent.js" } }),
      endpoint: "var r; try { openidm.create('managed/widget', 'w9', { text: 'x' }); r = 'created'; } catch (e) { r = 'caught'; } ({ r: r });" });
    const result = await emu.request({ method: "GET", path: "endpoint/probe" });
    expect(result.status).toBe(501);
  });

  it("an id that leaves the script root is its own fault", async () => {
    expectHarnessFault("idm-runtime-script-path");
    const emu = emulator({ runtimeScripts: directory, endpoint: "try { require('../outside'); } catch (e) {} ({});" });
    expect((await emu.request({ method: "GET", path: "endpoint/probe" })).status).toBe(501);
  });
});

describe("provider configuration", () => {
  it("refuses a provider of the wrong shape", () => {
    expect(() => emulator({ runtimeScripts: 42 as unknown as string })).toThrow(/runtimeScripts/);
    expect(() => emulator({ runtimeScripts: "" })).toThrow(/runtimeScripts/);
  });
});
