/** Built-in node types measured on a tenant 2026-11-25; docs/api/25-journey-runtime.md "Built-in node types". */
import { afterAll, beforeAll, expect, it } from "vitest";
import { SUCCESS_NODE_ID as SUCCESS } from "../../src/aic/constants.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import {
  JourneyConfig,
  JourneyEmulator,
  type JourneyConfigInput,
  type JourneyOptions,
} from "../../src/journey/index.ts";
import type { JsonValue } from "../../src/protocol.ts";
let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
});
const script = (source: string) => ({
  evaluatorVersion: "2.0",
  script: Buffer.from(source).toString("base64"),
});
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
  for (const [id, name, password] of [
    [ALICE, "alice", "alice-secret"],
    [BOB, "bob", "bob-secret"],
  ] as const)
    await idm.request({
      method: "PUT",
      path: `managed/alpha_user/${id}`,
      body: {
        userName: name,
        mail: `${name}@example.com`,
        password,
        accountStatus: "active",
      },
    });
  return idm;
}
/**
 * seed -> node -> one dump script per branch. A dump reports the branch and the state the node left, as a
 * HiddenValueCallback (which also ends the request, so the next request is a round trip).
 */
function flow(
  type: string,
  node: Record<string, unknown>,
  seed: string,
  branches: string[] = ["outcome"],
  options: { success?: boolean } = {},
): JourneyConfigInput {
  const report = (branch: string) =>
    `callbacksBuilder.hiddenValueCallback('state',JSON.stringify({branch:${JSON.stringify(branch)},keys:nodeState.keys().toArray?nodeState.keys().toArray().sort():null,` +
    `id:nodeState.get('_id'),username:nodeState.get('username'),otp:nodeState.get('oneTimePassword'),` +
    `ts:nodeState.get('oneTimePasswordTimestamp'),retries:nodeState.get('retries'),terms:nodeState.get('termsAccepted'),` +
    `termsVersion:nodeState.get('termsVersion'),pass:callbacks.isEmpty()?1:2}));`;
  return {
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: "seed",
        identityResource: "managed/alpha_user",
        nodes: {
          seed: {
            nodeType: "ScriptedDecisionNode",
            connections: { next: "node" },
          },
          node: {
            nodeType: type,
            connections: Object.fromEntries(
              branches.map((branch) => [branch, options.success ? SUCCESS : `dump-${branch}`]),
            ),
          },
          ...Object.fromEntries(
            branches.map((branch) => [
              `dump-${branch}`,
              { nodeType: "ScriptedDecisionNode", connections: {} },
            ]),
          ),
        },
      },
    },
    nodes: {
      [type]: { node },
      ScriptedDecisionNode: {
        seed: { script: "seed", outcomes: ["next"] },
        ...Object.fromEntries(
          branches.map((branch) => [`dump-${branch}`, { script: `dump-${branch}`, outcomes: [] }]),
        ),
      },
    },
    scripts: {
      seed: script(seed + ";action.goTo('next');"),
      ...Object.fromEntries(branches.map((b) => [`dump-${b}`, script(report(b))])),
    },
  };
}
function make(input: JourneyConfigInput, options: JourneyOptions = {}, idm?: IdmEmulator) {
  return new JourneyEmulator(
    new JourneyConfig(input),
    runner,
    idm ??
      new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] } }), runner),
    options,
  );
}
function request(emu: JourneyEmulator, body: JsonValue = {}) {
  return emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "invented" },
    body,
  });
}
function dump(body: JsonValue): Record<string, unknown> {
  return JSON.parse(
    String(
      (
        body as {
          callbacks: { output: { name: string; value: JsonValue }[] }[];
        }
      ).callbacks[0]?.output.find((field) => field.name === "value")?.value,
    ),
  ) as Record<string, unknown>;
}
const LOGIN_FAILURE = {
  code: 401,
  reason: "Unauthorized",
  message: "Login failure",
};
const put = (shared: Record<string, string>, transient: Record<string, string> = {}) =>
  [
    ...Object.entries(shared).map(([k, v]) => `nodeState.putShared('${k}','${v}');`),
    ...Object.entries(transient).map(([k, v]) => `nodeState.putTransient('${k}','${v}');`),
  ].join("");

// DataStoreDecisionNode (docs/api/25: measured with a username and password put in state).
it.each([
  // [name, shared, transient, branch]
  ["right password", { username: "alice" }, { password: "alice-secret" }, "true"],
  // Discriminating: a lookup that ignores letter case, like the tenant's.
  ["username in another case", { username: "ALiCe" }, { password: "alice-secret" }, "true"],
  // Discriminating: a check that only finds the user would accept this.
  ["wrong password", { username: "alice" }, { password: "nope" }, "false"],
  // Discriminating: another user's password is still wrong for this one.
  ["another user's password", { username: "alice" }, { password: "bob-secret" }, "false"],
  ["unknown user", { username: "nobody" }, { password: "alice-secret" }, "false"],
  ["empty password", { username: "alice" }, { password: "" }, "false"],
  // Discriminating: the lookup is userName only, so a mail address is not a username.
  ["mail as username", { username: "alice@example.com" }, { password: "alice-secret" }, "false"],
  ["password in shared state", { username: "alice", password: "alice-secret" }, {}, "true"],
] as const)("DataStoreDecisionNode: %s", async (_name, shared, transient, branch) => {
  const emu = make(
    flow("DataStoreDecisionNode", {}, put(shared, transient), ["true", "false"]),
    {},
    await users(),
  );
  const result = await request(emu);
  expect(result.status).toBe(200);
  const state = dump(result.body);
  expect(state.branch).toBe(branch);
  // Measured: no _id, no username rewrite, no authLevel change.
  expect(state.id).toBeNull();
  expect(state.username).toBe(shared.username);
});
it.each([
  ["no username", {}, { password: "alice-secret" }],
  // Discriminating: a username only in transient state is not read, though a password in shared state is.
  ["username only transient", {}, { username: "alice", password: "alice-secret" }],
  ["no password", { username: "alice" }, {}],
] as const)(
  "DataStoreDecisionNode aborts with 401 Login failure: %s",
  async (_n, shared, transient) => {
    const emu = make(
      flow("DataStoreDecisionNode", {}, put(shared, transient), ["true", "false"]),
      {},
      await users(),
    );
    expect(await request(emu)).toMatchObject({
      status: 401,
      body: LOGIN_FAILURE,
    });
  },
);
it("DataStoreDecisionNode takes the password the user has now, not the first one", async () => {
  const idm = await users();
  await idm.request({
    method: "PATCH",
    path: `managed/alpha_user/${ALICE}`,
    body: [{ operation: "replace", field: "/password", value: "changed-secret" }],
  });
  const branch = async (password: string) =>
    dump(
      (
        await request(
          make(
            flow("DataStoreDecisionNode", {}, put({ username: "alice" }, { password }), [
              "true",
              "false",
            ]),
            {},
            idm,
          ),
        )
      ).body,
    ).branch;
  expect(await branch("changed-secret")).toBe("true");
  expect(await branch("alice-secret")).toBe("false");
});

// SetSessionPropertiesNode and SetPersistentCookieNode.
it("SetSessionPropertiesNode puts properties and timeouts on the session of a successful journey", async () => {
  const input = flow(
    "SetSessionPropertiesNode",
    {
      properties: { first: "1", second: "2" },
      maxIdleTime: 20,
      maxSessionTime: 60,
    },
    put({ username: "alice" }),
    ["outcome"],
    { success: true },
  );
  const result = await request(make(input));
  expect(result.status).toBe(200);
  expect(result.session).toEqual({
    properties: { first: "1", second: "2" },
    maxIdleTime: 20,
    maxSessionTime: 60,
  });
  expect(result.cookies).toBeUndefined();
});
it("SetSessionPropertiesNode with only timeouts sets no properties", async () => {
  const input = flow(
    "SetSessionPropertiesNode",
    { maxIdleTime: 7 },
    put({ username: "alice" }),
    ["outcome"],
    { success: true },
  );
  expect((await request(make(input))).session).toEqual({
    properties: {},
    maxIdleTime: 7,
  });
});
it.each([
  [{}, "At least one of the following properties must be set"],
  [{ properties: {} }, "At least one of the following properties must be set"],
  // Discriminating: a tenant-owned name is refused even beside an allowed one.
  [{ properties: { fine: "x", AMCtxId: "x" } }, "cannot include system session properties"],
  [{ properties: { CharSet: "x" } }, "cannot include system session properties"],
])("SetSessionPropertiesNode config %j is refused at load", (node, message) => {
  expect(() => new JourneyConfig(flow("SetSessionPropertiesNode", node, ""))).toThrow(message);
});
it("SetSessionPropertiesNode accepts a non-system property that merely looks like one", () => {
  expect(
    () =>
      new JourneyConfig(
        flow(
          "SetSessionPropertiesNode",
          { properties: { amlbcookie2: "x", authinstant: "x" } },
          "",
        ),
      ),
  ).not.toThrow();
});
const NOW = Date.UTC(2026, 10, 25, 1, 2, 3);
const KEY = Buffer.alloc(32, 7).toString("base64");
it.each([
  [
    "defaults",
    {
      persistentCookieName: "remember",
      maxLife: 9,
      sameSite: "LAX",
      hmacSigningKey: KEY,
    },
    {
      name: "remember",
      httpOnly: true,
      sameSite: "lax",
      expires: "Wed, 25 Nov 2026 10:02:03 GMT",
    },
  ],
  [
    "not HttpOnly, strict",
    {
      persistentCookieName: "c2",
      maxLife: 3,
      sameSite: "STRICT",
      useHttpOnlyCookie: false,
      useSecureCookie: false,
      hmacSigningKey: KEY,
    },
    {
      name: "c2",
      httpOnly: false,
      sameSite: "strict",
      expires: "Wed, 25 Nov 2026 04:02:03 GMT",
    },
  ],
  [
    "none, useSecureCookie false",
    {
      persistentCookieName: "c3",
      maxLife: 3,
      sameSite: "NONE",
      useSecureCookie: false,
      hmacSigningKey: KEY,
    },
    {
      name: "c3",
      httpOnly: true,
      sameSite: "none",
      expires: "Wed, 25 Nov 2026 04:02:03 GMT",
    },
  ],
] as const)("SetPersistentCookieNode: %s", async (_name, node, expected) => {
  const input = flow("SetPersistentCookieNode", node, put({ username: "alice" }), ["outcome"], {
    success: true,
  });
  const result = await request(make(input, { now: () => NOW }));
  expect(result.status).toBe(200);
  expect(result.cookies).toHaveLength(1);
  // Discriminating: Secure is set even where useSecureCookie is false (the tenant forces it).
  expect(result.cookies?.[0]).toMatchObject({
    ...expected,
    secure: true,
    path: "/",
  });
  expect(result.cookies?.[0]?.value.split(".")).toHaveLength(3);
});
it("SetPersistentCookieNode with no signing key fails the journey with an empty 500", async () => {
  const input = flow(
    "SetPersistentCookieNode",
    { persistentCookieName: "remember", maxLife: 1 },
    put({ username: "alice" }),
    ["outcome"],
    { success: true },
  );
  expect(await request(make(input))).toMatchObject({
    status: 500,
    body: null,
    rawBody: "",
  });
});

// OneTimePasswordGeneratorNode.
it.each([6, 7, 8])(
  "OneTimePasswordGeneratorNode length %i writes digits and an epoch-seconds stamp",
  async (length) => {
    const emu = make(flow("OneTimePasswordGeneratorNode", { length }, ""), {
      now: () => NOW,
    });
    const seen = new Set<string>();
    for (let run = 0; run < 40; run++) {
      const state = dump((await request(emu)).body);
      expect(state.otp).toMatch(new RegExp(`^[0-9]{${length}}$`));
      // Discriminating: seconds, not milliseconds.
      expect(state.ts).toBe(Math.floor(NOW / 1000));
      seen.add(String(state.otp));
    }
    // A constant "password" would pass the shape checks.
    expect(seen.size).toBeGreaterThan(1);
  },
);
it("OneTimePasswordGeneratorNode keeps leading zeros as a string", async () => {
  const emu = make(flow("OneTimePasswordGeneratorNode", { length: 6 }, ""));
  // Chance of never drawing a leading zero in 150 runs: 0.9^150, about 1e-7.
  let zeros = 0;
  for (let run = 0; run < 150; run++) {
    const state = dump((await request(emu)).body);
    expect(typeof state.otp).toBe("string");
    if (String(state.otp).startsWith("0")) zeros++;
  }
  expect(zeros).toBeGreaterThan(0);
}, 60_000);
it("OneTimePasswordGeneratorNode state is shared, so it survives the round trip", async () => {
  const emu = make(flow("OneTimePasswordGeneratorNode", { length: 6 }, ""));
  const first = await request(emu);
  const state = dump(first.body);
  expect(dump((await request(emu, first.body)).body)).toMatchObject({
    otp: state.otp,
    ts: state.ts,
    pass: 2,
  });
});
it("OneTimePasswordGeneratorNode defaults to 8 digits", async () => {
  expect(
    String(dump((await request(make(flow("OneTimePasswordGeneratorNode", {}, "")))).body).otp),
  ).toHaveLength(8);
});
it.each([9, 10, 20])(
  "OneTimePasswordGeneratorNode length %i fails the journey at run time",
  async (length) => {
    const emu = make(flow("OneTimePasswordGeneratorNode", { length }, ""));
    expect(await request(emu)).toMatchObject({
      status: 401,
      body: LOGIN_FAILURE,
    });
  },
);
it.each([0, 3, 5])("OneTimePasswordGeneratorNode length %i is refused at load", (length) => {
  expect(() => new JourneyConfig(flow("OneTimePasswordGeneratorNode", { length }, ""))).toThrow(
    "One Time Password Length",
  );
});

// RetryLimitDecisionNode.
function retryFlow(limit: number | undefined, nodes = 1): JourneyConfigInput {
  const base = flow(
    "RetryLimitDecisionNode",
    limit === undefined ? {} : { retryLimit: limit },
    "",
    ["Retry", "Reject"],
  );
  // Retry loops back to the node through a counter, with no callback, so one request runs the whole loop.
  const tree = base.trees.invented;
  if (!tree) throw new Error("fixture");
  const nodesMap: Record<string, unknown> = { ...tree.nodes };
  nodesMap.node = {
    nodeType: "RetryLimitDecisionNode",
    connections: {
      Retry: "again",
      Reject: nodes === 2 ? "second" : "dump-Reject",
    },
  };
  nodesMap.again = {
    nodeType: "ScriptedDecisionNode",
    connections: { next: "node" },
  };
  const out: JourneyConfigInput = {
    ...base,
    trees: { invented: { ...tree, nodes: nodesMap as typeof tree.nodes } },
  };
  (out.nodes.ScriptedDecisionNode as Record<string, unknown>).again = {
    script: "again",
    outcomes: ["next"],
  };
  (out.scripts as Record<string, unknown>).again = script(
    "nodeState.putShared('retries',(nodeState.get('retries')||0)+1);action.goTo('next');",
  );
  return out;
}
it.each([
  [1, 1],
  [2, 2],
  [3, 3],
  [5, 5],
])(
  "RetryLimitDecisionNode retryLimit %i takes Retry exactly %i times, then Reject",
  async (limit, retries) => {
    const state = dump((await request(make(retryFlow(limit)))).body);
    // Discriminating: an off-by-one limit takes Retry limit-1 or limit+1 times.
    expect(state).toMatchObject({ branch: "Reject", retries });
  },
);
it("RetryLimitDecisionNode defaults to 3 and counts in shared <nodeId>.retryCount, which stops at the limit", async () => {
  const input = retryFlow(undefined);
  const tree = input.trees.invented;
  if (!tree) throw new Error("fixture");
  (input.scripts as Record<string, unknown>)["dump-Reject"] = script(
    "callbacksBuilder.hiddenValueCallback('state',JSON.stringify({retries:nodeState.get('retries'),count:nodeState.get('node.retryCount'),type:typeof nodeState.get('node.retryCount')}));",
  );
  const state = dump((await request(make(input))).body);
  expect(state).toEqual({ retries: 3, count: 3, type: "number" });
});
it("RetryLimitDecisionNode refuses retryLimit 0 at load", () => {
  expect(() => new JourneyConfig(retryFlow(0))).toThrow("The value must be greater than zero");
});
it("RetryLimitDecisionNode writes nothing to the identified user, whatever incrementUserAttributeOnFailure says", async () => {
  const idm = await users();
  const before = await idm.request({
    method: "GET",
    path: `managed/alpha_user/${ALICE}`,
  });
  const input = retryFlow(2);
  (input.nodes.RetryLimitDecisionNode as Record<string, unknown>).node = {
    retryLimit: 2,
    incrementUserAttributeOnFailure: true,
  };
  const seed = put({ username: "alice", _id: ALICE });
  (input.scripts as Record<string, unknown>).seed = script(seed + ";action.goTo('next');");
  expect(dump((await request(make(input, {}, idm))).body)).toMatchObject({
    branch: "Reject",
  });
  expect(await idm.request({ method: "GET", path: `managed/alpha_user/${ALICE}` })).toMatchObject({
    body: before.body,
  });
});

// AcceptTermsAndConditionsNode and the CAPTCHA stub.
function answerWith(body: JsonValue, value: JsonValue): JsonValue {
  const copy = structuredClone(body) as { callbacks: { input: { value: JsonValue }[] }[] };
  const input = copy.callbacks[0]?.input[0];
  if (!input) throw new Error("no callback input");
  input.value = value;
  return copy as JsonValue;
}
type Wire = {
  callbacks: {
    type: string;
    output: { name: string; value: JsonValue }[];
    input: { name: string; value: JsonValue }[];
  }[];
};
const TERMS = { version: "9.1", terms: "Invented terms.", createDate: "2020-01-02T03:04:05.678Z" };
it("AcceptTermsAndConditionsNode sends the realm's terms with a false input", async () => {
  const emu = make(flow("AcceptTermsAndConditionsNode", {}, ""), { termsAndConditions: TERMS });
  const first = await request(emu);
  expect(first.status).toBe(200);
  expect((first.body as Wire).callbacks).toEqual([
    {
      type: "TermsAndConditionsCallback",
      output: [
        { name: "version", value: "9.1" },
        { name: "terms", value: "Invented terms." },
        { name: "createDate", value: "2020-01-02T03:04:05.678Z" },
      ],
      input: [{ name: "IDToken1", value: false }],
    },
  ]);
});
it.each([false, "false", ""])(
  "AcceptTermsAndConditionsNode sends the terms again for %j and records nothing",
  async (value) => {
    const emu = make(flow("AcceptTermsAndConditionsNode", {}, ""), { termsAndConditions: TERMS });
    const first = await request(emu);
    const next = await request(emu, answerWith(first.body, value));
    expect(next.status).toBe(200);
    expect((next.body as Wire).callbacks[0]?.type).toBe("TermsAndConditionsCallback");
    // Not the dump: no state was written either.
    expect((next.body as Wire).callbacks[0]?.output.map((field) => field.name)).toEqual([
      "version",
      "terms",
      "createDate",
    ]);
  },
);
// Discriminating: the boolean and the string both accept, and both write the same two keys.
it.each([true, "true"])("AcceptTermsAndConditionsNode accepts %j", async (value) => {
  const emu = make(flow("AcceptTermsAndConditionsNode", {}, ""), {
    termsAndConditions: TERMS,
    now: () => NOW,
  });
  const first = await request(emu);
  const next = await request(emu, answerWith(first.body, value));
  expect(dump(next.body)).toMatchObject({
    branch: "outcome",
    terms: '{ "acceptDate": "2026-11-25T01:02:03.000000000Z", "termsVersion": "9.1" }',
    termsVersion: "9.1",
  });
  // Shared state: still there after the round trip.
  expect(dump((await request(emu, next.body)).body)).toMatchObject({
    termsVersion: "9.1",
    pass: 2,
  });
});
it("AcceptTermsAndConditionsNode without the realm's terms is missing", async () => {
  const cause = "AcceptTermsAndConditionsNode needs JourneyOptions.termsAndConditions";
  expectEmulationFailure(cause);
  const result = await request(make(flow("AcceptTermsAndConditionsNode", {}, "")));
  expect(result).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});
const CAPTCHA = {
  siteKey: "invented-site-key",
  apiUri: "https://captcha.example.com/api.js",
  divClass: "invented-div",
  disableSubmission: true,
  reCaptchaV3: false,
};
it("product-CaptchaNode sends a ReCaptchaCallback built from the node", async () => {
  const emu = make(flow("product-CaptchaNode", CAPTCHA, "", ["true", "false"]));
  const first = await request(emu);
  expect((first.body as Wire).callbacks).toEqual([
    {
      type: "ReCaptchaCallback",
      output: [
        { name: "recaptchaSiteKey", value: "invented-site-key" },
        { name: "captchaApiUri", value: "https://captcha.example.com/api.js" },
        { name: "captchaDivClass", value: "invented-div" },
        { name: "reCaptchaV3", value: false },
        { name: "disableSubmission", value: true },
      ],
      input: [{ name: "IDToken1", value: "" }],
    },
  ]);
});
it.each(["true", "false"])("product-CaptchaNode takes the declared outcome %s", async (outcome) => {
  const emu = make(flow("product-CaptchaNode", CAPTCHA, "", ["true", "false"]), {
    declaredOutcomes: { node: outcome },
  });
  const first = await request(emu);
  expect(dump((await request(emu, answerWith(first.body, "any-token"))).body).branch).toBe(outcome);
});
it("product-CaptchaNode without a declared verdict is missing, not guessed", async () => {
  const cause =
    "product-CaptchaNode verification outcome; declare it in JourneyOptions.declaredOutcomes";
  expectEmulationFailure(cause);
  const emu = make(flow("product-CaptchaNode", CAPTCHA, "", ["true", "false"]));
  const first = await request(emu);
  expect(await request(emu, answerWith(first.body, "any-token"))).toMatchObject({
    status: 501,
    emulation: { missing: [cause] },
  });
});
it("product-CaptchaNode v3 is missing", async () => {
  expectEmulationFailure("product-CaptchaNode reCaptchaV3: true");
  const emu = make(
    flow("product-CaptchaNode", { ...CAPTCHA, reCaptchaV3: true }, "", ["true", "false"]),
  );
  expect(await request(emu)).toMatchObject({ status: 501 });
});
