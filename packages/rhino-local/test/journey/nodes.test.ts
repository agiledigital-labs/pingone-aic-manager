import { afterAll, beforeAll, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { JourneyConfig, JourneyEmulator, type JourneyConfigInput, type JourneyOptions } from "../../src/journey/index.ts";
import type { JsonValue } from "../../src/protocol.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
const script = (source: string) => ({ evaluatorVersion: "2.0", script: Buffer.from(source).toString("base64") });
function config(children: string[], source: string, outcomes = ["chosen"]): JourneyConfigInput {
  return { realm: "alpha", trees: { invented: { entryNodeId: "page", nodes: { page: { nodeType: "PageNode", connections: { outcome: "dump", chosen: "dump" } }, dump: { nodeType: "ScriptedDecisionNode", connections: {} } } } }, nodes: { PageNode: { page: { nodes: children.map((id) => ({ _id: id, nodeType: id === "user" ? "UsernameCollectorNode" : id === "pass" ? "PasswordCollectorNode" : "ScriptedDecisionNode" })) } }, UsernameCollectorNode: { user: {} }, PasswordCollectorNode: { pass: {} }, ScriptedDecisionNode: { child: { script: "child", outcomes }, dump: { script: "dump", outcomes: [] } } }, scripts: { child: script(source), dump: script("callbacksBuilder.hiddenValueCallback('state',JSON.stringify({username:nodeState.get('username'),password:nodeState.get('password'),seen:nodeState.get('seen'),passes:nodeState.get('passes'),page:nodeState.isDefined('pageNodeCallbacks')}));") } };
}
function make(input: JourneyConfigInput, options: JourneyOptions = {}, idm = new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner)) { return new JourneyEmulator(new JourneyConfig(input), runner, idm, options); }
function request(emu: JourneyEmulator, body: JsonValue = {}) { return emu.request({ method: "POST", path: "authenticate", query: { authIndexType: "service", authIndexValue: "invented" }, body }); }
function answer(body: JsonValue, values: string[]): JsonValue {
  const copy = structuredClone(body) as { callbacks: { input?: { value: JsonValue }[] }[] };
  copy.callbacks.forEach((cb, index) => { if (cb.input?.[0]) cb.input[0].value = values[index] ?? ""; });
  return copy as JsonValue;
}
function dump(body: JsonValue): unknown { return JSON.parse(String((body as { callbacks: { output: { name: string; value: JsonValue }[] }[] }).callbacks[0]?.output.find((field) => field.name === "value")?.value)); }
const child = "if(callbacks.isEmpty()){callbacksBuilder.textOutputCallback(0,'notice');callbacksBuilder.nameCallback('Script name');action.goTo('chosen');}else{nodeState.putShared('seen',{username:nodeState.get('username'),password:nodeState.get('password'),own:String(callbacks.getNameCallbacks().get(0))});action.goTo('chosen');}";
it.each([false, true])("Page order, callback isolation and sequential state: script last=%s", async (last) => {
  const emu = make(config(last ? ["user", "pass", "child"] : ["user", "child", "pass"], child));
  const first = await request(emu);
  expect(first.status).toBe(200);
  const callbacks = (first.body as { callbacks: { type: string; input?: { name: string }[] }[] }).callbacks;
  expect(callbacks.map((cb) => cb.type)).toEqual(last ? ["NameCallback", "PasswordCallback", "TextOutputCallback", "NameCallback"] : ["NameCallback", "TextOutputCallback", "NameCallback", "PasswordCallback"]);
  expect(callbacks.at(-1)?.input?.[0]?.name).toBe("IDToken4");
  const next = await request(emu, answer(first.body, last ? ["invented", "secret", "", "own"] : ["invented", "", "own", "secret"]));
  expect(next.status).toBe(200);
  expect(dump(next.body)).toEqual({ username: "invented", password: "secret", passes: null, seen: { username: "invented", password: last ? "secret" : null, own: "own" }, page: true });
  const later = await request(emu, next.body);
  expect(dump(later.body)).toMatchObject({ username: "invented", password: null, page: true });
});
it("Page rejects a non-last multi-outcome child at config construction", () => {
  expect(() => new JourneyConfig(config(["child", "pass"], child, ["a", "b"]))).toThrow("Invalid config: Only the last node in a page can have more than one outcome");
});
it("a declared outcome cannot bypass an emulated Page", () => {
  expect(() => make(config(["user", "pass"], child), { declaredOutcomes: { page: "outcome" } })).toThrow("declared outcome for emulated node");
});
