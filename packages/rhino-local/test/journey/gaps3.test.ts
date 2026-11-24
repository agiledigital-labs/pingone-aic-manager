/** Journey emulator gaps, round 3: found by rewriting real journeys; generic trees only. */
import { createHmac } from "node:crypto";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { JourneyConfig, JourneyEmulator, type JourneyOptions } from "../../src/journey/index.ts";
import type { JourneyConfigInput } from "../../src/journey/index.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import type { JsonValue } from "../../src/protocol.ts";
import { SUCCESS_NODE_ID as SUCCESS } from "../../src/aic/constants.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});

const b64 = (source: string) => Buffer.from(source).toString("base64");
const script = (source: string) => ({ evaluatorVersion: "2.0", script: b64(source) });
const ALICE = "00000000-0000-0000-0000-0000000000a1";
const BOB = "00000000-0000-0000-0000-0000000000b2";
async function users(): Promise<IdmEmulator> {
  const idm = new IdmEmulator(
    new IdmConfig({
      sync: { mappings: [] },
      managed: {
        objects: [
          {
            name: "alpha_user",
            schema: {
              properties: {
                userName: { type: "string", title: "Username" },
                mail: { type: "string", title: "Email Address" },
                password: { type: "string", title: "Password" },
                accountStatus: { type: "string", title: "Status" },
              },
            },
          },
        ],
      },
    }),
    runner,
  );
  for (const [id, name] of [
    [ALICE, "alice"],
    [BOB, "bob"],
  ] as const)
    await idm.request({
      method: "PUT",
      path: `managed/alpha_user/${id}`,
      body: { userName: name, mail: `${name}@example.com`, password: `${name}-secret`, accountStatus: "active" },
    });
  return idm;
}
function make(input: JourneyConfigInput, options: JourneyOptions = {}, idm?: IdmEmulator) {
  return new JourneyEmulator(
    new JourneyConfig(input),
    runner,
    idm ?? new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }), runner),
    options,
  );
}
const run = (emu: JourneyEmulator, tree: string, extra: Record<string, unknown> = {}) =>
  emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: tree },
    body: {},
    ...extra,
  });
/** The JSON a state-dump script put in its first callback. */
function dump(body: JsonValue): Record<string, unknown> {
  const callbacks = (body as { callbacks?: { output: { name: string; value: JsonValue }[] }[] }).callbacks;
  return JSON.parse(String(callbacks?.[0]?.output.find((field) => field.name === "value")?.value)) as Record<
    string,
    unknown
  >;
}
const report = (branch: string) =>
  `callbacksBuilder.hiddenValueCallback('state',JSON.stringify({branch:'${branch}',username:nodeState.get('username')}));`;

// ---------------------------------------------------------------------------------------------
// Persistent cookies (batch 4 G23, G26, G30).
describe("persistent cookie", () => {
  const KEY = Buffer.from("invented-persistent-key-0123456789").toString("base64");
  const OTHER_KEY = Buffer.from("another-invented-key-0123456789ab").toString("base64");
  const COOKIE = { persistentCookieName: "inventedCookie", hmacSigningKey: KEY, maxLife: 3 };
  function journey(name: string, idm: IdmEmulator, options: JourneyOptions = {}, reader: Record<string, unknown> = {}) {
    return make(
      {
        realm: "alpha",
        trees: {
          setter: {
            entryNodeId: "seed",
            nodes: {
              seed: { nodeType: "ScriptedDecisionNode", connections: { next: "set" } },
              set: { nodeType: "SetPersistentCookieNode", connections: { outcome: SUCCESS } },
            },
          },
          reader: {
            entryNodeId: "read",
            identityResource: "managed/alpha_user",
            nodes: {
              read: { nodeType: "PersistentCookieDecisionNode", connections: { true: "yes", false: "no" } },
              yes: { nodeType: "ScriptedDecisionNode", connections: {} },
              no: { nodeType: "ScriptedDecisionNode", connections: {} },
            },
          },
        },
        nodes: {
          ScriptedDecisionNode: {
            seed: { script: "seed", outcomes: ["next"] },
            yes: { script: "yes", outcomes: [] },
            no: { script: "no", outcomes: [] },
          },
          SetPersistentCookieNode: { set: COOKIE },
          PersistentCookieDecisionNode: { read: { ...COOKIE, idleTimeout: 5, enforceClientIp: false, ...reader } },
        },
        scripts: {
          seed: script(`nodeState.putShared('username','${name}');action.goTo('next');`),
          yes: script(report("true")),
          no: script(report("false")),
        },
      },
      options,
      idm,
    );
  }
  const issued = async (emu: JourneyEmulator): Promise<string> => {
    const cookie = (await run(emu, "setter")).cookies?.[0];
    expect(cookie?.name).toBe("inventedCookie");
    return cookie?.value ?? "";
  };
  const read = (emu: JourneyEmulator, value: string) =>
    run(emu, "reader", { cookies: { inventedCookie: value } });
  const sign = (header: string, payload: string, key = KEY) =>
    `${header}.${payload}.${createHmac("sha256", Buffer.from(key, "base64")).update(`${header}.${payload}`).digest("base64url")}`;
  const part = (text: string) => Buffer.from(text).toString("base64url");

  it("G26: a request cookie needs no cookieName; a known session's value still does", async () => {
    // No cookieName configured: the persistent cookie reaches the node. A wrong implementation
    // that keeps refusing any cookie without cookieName fails here.
    const emu = journey("alice", await users());
    const value = await issued(emu);
    expect(dump((await read(emu, value)).body)).toMatchObject({ branch: "true", username: ALICE });
    // Discriminating: one that ignores cookies altogether would silently drop an existing session.
    expectEmulationFailure("session cookie name not supplied");
    const sessions = journey("alice", await users(), { sessions: { "session-value": { AuthLevel: "0" } } });
    const result = await run(sessions, "reader", { cookies: { someCookie: "session-value" } });
    expect(result.status).toBe(501);
  });

  it("G23: a decision node's signing key wrapped as $string reads like the literal", async () => {
    const emu = journey("alice", await users(), {}, { hmacSigningKey: { $string: KEY } });
    const value = await issued(emu);
    expect(dump((await read(emu, value)).body).branch).toBe("true");
    // Discriminating: the wrapper object itself used as a key could not verify its own cookie, and a
    // different wrapped key must not verify this one.
    const other = journey("alice", await users(), {}, { hmacSigningKey: { $string: OTHER_KEY } });
    expect(dump((await read(other, value)).body).branch).toBe("false");
  });

  it("G30: the value is an HS256 JWS over a five-part JWE, not a plain JWT", async () => {
    const value = await issued(journey("alice", await users()));
    const [head, payload, signature] = value.split(".") as [string, string, string];
    expect(JSON.parse(Buffer.from(head, "base64url").toString())).toEqual({ typ: "JWT", cty: "JWT", alg: "HS256" });
    expect(signature).toBe(
      createHmac("sha256", Buffer.from(KEY, "base64")).update(`${head}.${payload}`).digest("base64url"),
    );
    const inner = Buffer.from(payload, "base64url").toString();
    expect(inner.split(".")).toHaveLength(5);
    expect(JSON.parse(Buffer.from(inner.split(".")[0] as string, "base64url").toString())).toMatchObject({
      enc: "A128CBC-HS256",
    });
    // Discriminating: the claims are not readable in the outer payload, as they are in a plain JWT.
    expect(inner).not.toContain("alice");
    expect(value).not.toContain(Buffer.from("alice").toString("base64url"));
  });

  it("G30: a cookie signed by hand with the node's key is not accepted", async () => {
    const emu = journey("alice", await users());
    const head = part('{"typ":"JWT","cty":"JWT","alg":"HS256"}');
    // Plain claims under the right signing key, which is what a test author would write: the tenant
    // aborts with 401 (measured), and so does the emulator.
    const claims = part(JSON.stringify({ sub: "alice", iat: 1, exp: 4_102_444_800 }));
    expect(await read(emu, sign(head, claims))).toMatchObject({ status: 401 });
    // Five dotted parts that are not a JWE this emulator sealed: false, measured likewise.
    expect(dump((await read(emu, sign(head, part("not.a.jwe.at.all")))).body).branch).toBe("false");
  });

  it("G30: the identity is the encrypted part's: re-signing another user's inner token names that user", async () => {
    const idm = await users();
    const emu = journey("alice", idm);
    const bobs = journey("bob", idm);
    const alicesValue = await issued(emu);
    const bobsInner = ((await issued(bobs)).split(".") as string[])[1] as string;
    const head = (alicesValue.split(".") as string[])[0] as string;
    // Measured on the tenant: the outer signature covers the signing key only; the user comes from the
    // inner token. A wrong implementation keeping `sub` in the signed outer payload would name Alice.
    const swapped = await read(emu, sign(head, bobsInner));
    expect(dump(swapped.body)).toMatchObject({ branch: "true", username: BOB });
  });

  it("G30: damaged and short values take the measured branch", async () => {
    const emu = journey("alice", await users());
    const value = await issued(emu);
    const [head, payload] = value.split(".") as [string, string];
    const inner = Buffer.from(payload, "base64url").toString().split(".");
    inner[3] = `${(inner[3] as string).slice(0, -2)}AA`;
    expect(dump((await read(emu, sign(head, part(inner.join("."))))).body).branch).toBe("false");
    expect(dump((await read(emu, `${head}.${payload}`)).body).branch).toBe("false");
    expect(dump((await read(emu, sign(head, payload, OTHER_KEY))).body).branch).toBe("false");
    expect(await read(emu, "")).toMatchObject({ status: 401 });
    expect(await read(emu, "notajwt")).toMatchObject({ status: 401 });
    expectEmulationFailure("PersistentCookieDecisionNode over a cookie of 4 dotted parts");
    expect((await read(emu, `${value}.extra`)).status).toBe(501);
  });
});
