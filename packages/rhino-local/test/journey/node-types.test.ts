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
import type { LogRecord } from "../../src/logs/record.ts";
import type { CustomNodeType } from "../../src/journey/extra-nodes.ts";
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
    `termsVersion:nodeState.get('termsVersion'),authLevel:nodeState.get('authLevel'),k1:nodeState.get('k1'),` +
    `oa:nodeState.get('objectAttributes')?String(nodeState.get('objectAttributes')):null,kOut:nodeState.get('kdemoOut'),pass:callbacks.isEmpty()?1:2}));`;
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
  const result = await request(make(input, { now: () => BigInt(NOW) * 1_000_000n }));
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
      now: () => BigInt(NOW) * 1_000_000n,
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
      _id: 0,
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
    now: () => BigInt(NOW) * 1_000_000n,
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
      _id: 0,
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
// Username/Password collectors (docs/api/25, 2026-11-25): an empty answer re-sends the callback, alone or in
// a Page; a whitespace-only answer is accepted.
function collectorFlow(page: boolean): JourneyConfigInput {
  const dumpSource = `callbacksBuilder.hiddenValueCallback('state',JSON.stringify({branch:'out',username:nodeState.get('username')}));`;
  const kids = [
    { _id: "user", nodeType: "UsernameCollectorNode", displayName: "u" },
    { _id: "pass", nodeType: "PasswordCollectorNode", displayName: "p" },
  ];
  return {
    realm: "alpha",
    trees: {
      invented: {
        entryNodeId: page ? "page" : "user",
        nodes: page
          ? {
              page: { nodeType: "PageNode", connections: { outcome: "dump" } },
              dump: { nodeType: "ScriptedDecisionNode", connections: {} },
            }
          : {
              user: { nodeType: "UsernameCollectorNode", connections: { outcome: "dump" } },
              dump: { nodeType: "ScriptedDecisionNode", connections: {} },
            },
      },
    },
    nodes: {
      ...(page ? { PageNode: { page: { nodes: kids } } } : {}),
      UsernameCollectorNode: { user: {} },
      PasswordCollectorNode: { pass: {} },
      ScriptedDecisionNode: { dump: { script: "dump", outcomes: [] } },
    },
    scripts: { dump: script(dumpSource) },
  };
}
const answers = (body: JsonValue, values: string[]): JsonValue => {
  const b = structuredClone(body) as {
    callbacks: { input: { name: string; value: JsonValue }[] }[];
  };
  b.callbacks.forEach((cb, i) => {
    const input = cb.input[0];
    if (input && values[i] !== undefined) input.value = values[i];
  });
  return b as unknown as JsonValue;
};
const types = (body: JsonValue): string[] =>
  (body as { callbacks: { type: string }[] }).callbacks.map((cb) => cb.type);
it("UsernameCollectorNode re-sends its callback on an empty answer, then accepts a real one", async () => {
  const emu = make(collectorFlow(false));
  const first = await request(emu);
  // Discriminating: the old behaviour accepted "" and ran the dump with username "".
  const empty = await request(emu, answers(first.body, [""]));
  expect(types(empty.body)).toEqual(["NameCallback"]);
  const ok = await request(emu, answers(empty.body, ["alice"]));
  expect(dump(ok.body).username).toBe("alice");
});
it("a whitespace-only username is accepted (not empty)", async () => {
  const emu = make(collectorFlow(false));
  const first = await request(emu);
  const spaces = await request(emu, answers(first.body, ["  "]));
  expect(dump(spaces.body).username).toBe("  ");
});
it.each([
  ["empty username", ["", "pw-1"]],
  ["empty password", ["alice", ""]],
  ["both empty", ["", ""]],
] as const)("a Page of collectors re-sends the whole Page: %s", async (_n, values) => {
  const emu = make(collectorFlow(true));
  const first = await request(emu);
  const again = await request(emu, answers(first.body, [...values]));
  expect(types(again.body)).toEqual(["NameCallback", "PasswordCallback"]);
  const ok = await request(emu, answers(again.body, ["alice", "pw-1"]));
  expect(dump(ok.body).username).toBe("alice");
});

const wire = (body: JsonValue) => (body as Wire).callbacks;
const answersAt = (body: JsonValue, index: number, value: JsonValue): JsonValue => {
  const copy = structuredClone(body) as { callbacks: { input: { value: JsonValue }[] }[] };
  const input = copy.callbacks[index]?.input[0];
  if (!input) throw new Error("no callback input");
  input.value = value;
  return copy as JsonValue;
};

// AnonymousUserNode / SetSuccessUrlNode (docs/api/25 "Built-in node types", 2026-11-25).
it.each([
  ["a configured name", { anonymousUserName: "guest-1" }, "guest-1"],
  // Discriminating: the default is the word anonymous, not an empty or absent name.
  ["the default name", {}, "anonymous"],
] as const)("AnonymousUserNode writes %s as username", async (_n, cfg, name) => {
  const result = await request(make(flow("AnonymousUserNode", cfg, "")));
  expect(dump(result.body)).toMatchObject({ branch: "outcome", username: name });
});
it("a journey that ends at AnonymousUserNode succeeds with the default success URL", async () => {
  const result = await request(
    make(flow("AnonymousUserNode", {}, "", ["outcome"], { success: true })),
  );
  expect(result.status).toBe(200);
  expect(result.body).toMatchObject({ successUrl: "/enduser/?realm=/alpha", realm: "/alpha" });
});
it.each([
  [
    "a configured URL",
    { successUrl: "https://app.example.com/after" },
    "https://app.example.com/after",
  ],
  // Discriminating: no URL configured is null in the response, not the default URL.
  ["no URL", {}, null],
] as const)("SetSuccessUrlNode with %s", async (_n, cfg, url) => {
  const result = await request(
    make(
      flow("SetSuccessUrlNode", cfg, put({ username: "alice" }), ["outcome"], { success: true }),
    ),
  );
  expect(result.status).toBe(200);
  expect((result.body as { successUrl: unknown }).successUrl).toBe(url);
});

// SetStateNode, ModifyAuthLevelNode, AuthLevelDecisionNode.
it("SetStateNode writes literal strings and overwrites an existing key", async () => {
  const result = await request(
    make(
      flow(
        "SetStateNode",
        { attributes: { k1: "v1", username: "has ${username}" } },
        put({ username: "alice" }),
      ),
    ),
  );
  // Discriminating: a templating implementation would print "has alice".
  expect(dump(result.body)).toMatchObject({ k1: "v1", username: "has ${username}" });
});
it.each([
  [5, 0, 5],
  // Discriminating: it adds to the level already there, and a negative increment subtracts.
  [3, 4, 7],
  [-2, 0, -2],
] as const)("ModifyAuthLevelNode increment %i over %i", async (inc, from, to) => {
  const result = await request(
    make(
      flow(
        "ModifyAuthLevelNode",
        { authLevelIncrement: inc },
        `nodeState.putShared('authLevel',${from});`,
      ),
    ),
  );
  expect(dump(result.body)).toMatchObject({ branch: "outcome", authLevel: to });
});
it.each([
  [0, 0, "true"],
  [5, 5, "true"], // at the requirement: true (not strictly greater)
  [5, 4, "false"],
  [5, 6, "true"],
  [1, 0, "false"],
  [0, -1, "false"],
  [-1, -1, "true"],
  [-1, -2, "false"],
] as const)("AuthLevelDecisionNode requirement %i, level %i is %s", async (req, level, branch) => {
  const result = await request(
    make(
      flow(
        "AuthLevelDecisionNode",
        { authLevelRequirement: req },
        `nodeState.putShared('authLevel',${level});`,
        ["true", "false"],
      ),
    ),
  );
  expect(dump(result.body).branch).toBe(branch);
});

// MessageNode.
it("MessageNode sends the text and a Yes/No confirmation, and answer 0 is true", async () => {
  const emu = make(
    flow(
      "MessageNode",
      { message: { en: "Hello" }, messageYes: { en: "Go" }, messageNo: { en: "Stop" } },
      "",
      ["true", "false"],
    ),
  );
  const first = await request(emu);
  expect(wire(first.body)).toEqual([
    {
      type: "TextOutputCallback",
      output: [
        { name: "message", value: "Hello" },
        { name: "messageType", value: "0" },
      ],
      _id: 0,
    },
    {
      type: "ConfirmationCallback",
      output: [
        { name: "prompt", value: "" },
        { name: "messageType", value: 0 },
        { name: "options", value: ["Go", "Stop"] },
        { name: "optionType", value: -1 },
        { name: "defaultOption", value: 1 },
      ],
      input: [{ name: "IDToken2", value: 0 }],
      _id: 1,
    },
  ]);
  const yes = await request(emu, answersAt(first.body, 1, 0));
  expect(dump(yes.body).branch).toBe("true");
});
it.each([
  [1, "false"],
  // Discriminating: any index but 0 is the negative answer, and the string "0" is yes.
  [7, "false"],
  ["0", "true"],
] as const)("MessageNode answer %s takes %s", async (value, branch) => {
  const emu = make(flow("MessageNode", {}, "", ["true", "false"]));
  const first = await request(emu);
  expect(JSON.stringify(wire(first.body))).toContain('"Default message"');
  const next = await request(emu, answersAt(first.body, 1, value));
  expect(dump(next.body).branch).toBe(branch);
});

// ChoiceCollectorNode.
const CHOICES = { choices: ["red", "green", "blue"], defaultChoice: "green", prompt: "Pick one" };
it("ChoiceCollectorNode sends the choices with the default's index", async () => {
  const first = await request(
    make(flow("ChoiceCollectorNode", CHOICES, "", ["red", "green", "blue"])),
  );
  expect(wire(first.body)).toEqual([
    {
      type: "ChoiceCallback",
      output: [
        { name: "prompt", value: "Pick one" },
        { name: "choices", value: ["red", "green", "blue"] },
        { name: "defaultChoice", value: 1 },
      ],
      input: [{ name: "IDToken1", value: 1 }],
      _id: 0,
    },
  ]);
});
it.each([
  [0, "red"],
  [2, "blue"],
  ["1", "green"], // the index may arrive as a string
] as const)("ChoiceCollectorNode answer %s takes %s", async (value, branch) => {
  const emu = make(flow("ChoiceCollectorNode", CHOICES, "", ["red", "green", "blue"]));
  const first = await request(emu);
  expect(dump((await request(emu, answerWith(first.body, value))).body).branch).toBe(branch);
});
it.each([3, -1, "x"] as const)("ChoiceCollectorNode answer %s fails with 401", async (value) => {
  const emu = make(flow("ChoiceCollectorNode", CHOICES, "", ["red", "green", "blue"]));
  const first = await request(emu);
  expect(await request(emu, answerWith(first.body, value))).toMatchObject({ status: 401 });
});
it("ChoiceCollectorNode: a default that is not a choice is index 0, and no prompt is a 401", async () => {
  const stray = await request(
    make(
      flow("ChoiceCollectorNode", { ...CHOICES, defaultChoice: "zzz" }, "", [
        "red",
        "green",
        "blue",
      ]),
    ),
  );
  expect(JSON.stringify(wire(stray.body))).toContain('{"name":"defaultChoice","value":0}');
  const { prompt: _p, ...noPrompt } = CHOICES;
  const none = await request(
    make(flow("ChoiceCollectorNode", noPrompt, "", ["red", "green", "blue"])),
  );
  expect(none).toMatchObject({ status: 401, body: LOGIN_FAILURE });
});

// ValidatedUsernameNode (validateInput: false only).
const VU = { usernameAttribute: "userName", validateInput: false, autocompleteValues: [] };
it("ValidatedUsernameNode (no validation) writes username and objectAttributes.userName", async () => {
  const emu = make(flow("ValidatedUsernameNode", VU, ""));
  const first = await request(emu);
  expect(wire(first.body).map((cb) => cb.type)).toEqual(["NameCallback"]);
  const next = await request(emu, answerWith(first.body, "ab"));
  expect(dump(next.body)).toMatchObject({ branch: "outcome", username: "ab" });
  expect(String(dump(next.body).oa)).toContain("ab");
});
it("ValidatedUsernameNode (no validation) takes an empty answer and writes nothing", async () => {
  // Discriminating: unlike UsernameCollectorNode, which re-sends its callback.
  const emu = make(flow("ValidatedUsernameNode", VU, ""));
  const first = await request(emu);
  const next = await request(emu, answerWith(first.body, ""));
  expect(dump(next.body)).toMatchObject({ branch: "outcome", username: null, oa: null });
});
it("ValidatedUsernameNode with validation is missing unless its outcome is declared", async () => {
  const cause =
    "ValidatedUsernameNode validateInput: true; declare its outcome in JourneyOptions.declaredOutcomes";
  expectEmulationFailure(cause);
  const input = flow("ValidatedUsernameNode", { ...VU, validateInput: true }, "");
  expect(await request(make(input))).toMatchObject({
    status: 501,
    emulation: { missing: [cause] },
  });
  const declared = make(input, { declaredOutcomes: { node: "outcome" } });
  expect(dump((await request(declared)).body).branch).toBe("outcome");
});

// OneTimePasswordCollectorDecisionNode. The seed's timestamp is a fixed epoch second and the
// emulator's clock (OTP_NOW) the same instant, so the paired JVM lanes seed identical state.
const OTP_EPOCH_S = 1_795_000_000;
const OTP_NOW = { now: () => BigInt(OTP_EPOCH_S) * 1_000_000_000n };
const otpSeed = (otp: string | null, agoSeconds: number | null) =>
  (otp === null ? "" : `nodeState.putShared('oneTimePassword','${otp}');`) +
  (agoSeconds === null
    ? ""
    : `nodeState.putShared('oneTimePasswordTimestamp',${OTP_EPOCH_S - agoSeconds});`);
it.each([
  ["the right password", "123456", 10, "123456", "true"],
  ["a wrong password", "123456", 10, "654321", "false"],
  // Discriminating: leading zeros are significant, so a number comparison would call this right.
  ["a password with its leading zero dropped", "012345", 10, "12345", "false"],
  ["an expired password", "123456", 400, "123456", "false"],
  ["a password still inside the window", "123456", 290, "123456", "true"],
  ["no password in state", null, 10, "123456", "false"],
  ["no timestamp in state", "123456", null, "123456", "false"],
] as const)("OneTimePasswordCollectorDecisionNode: %s", async (_n, otp, ago, answer, branch) => {
  const emu = make(
    flow("OneTimePasswordCollectorDecisionNode", { passwordExpiryTime: 5 }, otpSeed(otp, ago), [
      "true",
      "false",
    ]),
    OTP_NOW,
  );
  const first = await request(emu);
  expect(wire(first.body)).toEqual([
    {
      type: "PasswordCallback",
      output: [{ name: "prompt", value: "One Time Password" }],
      input: [{ name: "IDToken1", value: "" }],
      _id: 0,
    },
  ]);
  expect(dump((await request(emu, answerWith(first.body, answer))).body).branch).toBe(branch);
});
it("OneTimePasswordCollectorDecisionNode re-sends on an empty answer and honours a one minute window", async () => {
  const emu = make(
    flow("OneTimePasswordCollectorDecisionNode", { passwordExpiryTime: 1 }, otpSeed("123456", 90), [
      "true",
      "false",
    ]),
    OTP_NOW,
  );
  const first = await request(emu);
  const empty = await request(emu, answerWith(first.body, ""));
  expect(wire(empty.body).map((cb) => cb.type)).toEqual(["PasswordCallback"]);
  // 90 s old against a 1 minute window: expired. (A 5 minute window accepts it, above.)
  expect(dump((await request(emu, answerWith(empty.body, "123456"))).body).branch).toBe("false");
});

// PersistentCookieDecisionNode, reading the cookie SetPersistentCookieNode issued.
const COOKIE_KEY = Buffer.from("invented-persistent-key-0123456789").toString("base64");
const COOKIE = { persistentCookieName: "inventedCookie", hmacSigningKey: COOKIE_KEY, maxLife: 3 };
function cookieJourney(name: string, idm: IdmEmulator) {
  const report = (branch: string) =>
    `callbacksBuilder.hiddenValueCallback('state',JSON.stringify({branch:'${branch}',username:nodeState.get('username')}));`;
  const emu = new JourneyEmulator(
    new JourneyConfig({
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
            read: {
              nodeType: "PersistentCookieDecisionNode",
              connections: { true: "yes", false: "no" },
            },
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
        PersistentCookieDecisionNode: {
          read: { ...COOKIE, idleTimeout: 5, enforceClientIp: false },
        },
      },
      scripts: {
        seed: script(`nodeState.putShared('username','${name}');action.goTo('next');`),
        yes: script(report("true")),
        no: script(report("false")),
      },
    }),
    runner,
    idm,
    { cookieName: "sessionCookie" },
  );
  return emu;
}
async function issued(emu: JourneyEmulator): Promise<string> {
  const set = await emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "setter" },
    body: {},
  });
  const cookie = set.cookies?.[0];
  expect(cookie?.name).toBe("inventedCookie");
  return cookie?.value ?? "";
}
const read = (emu: JourneyEmulator, cookies: Record<string, string>) =>
  emu.request({
    method: "POST",
    path: "authenticate",
    query: { authIndexType: "service", authIndexValue: "reader" },
    body: {},
    cookies,
  });
it("PersistentCookieDecisionNode takes true for its own cookie and false for the altered, absent and foreign", async () => {
  const emu = cookieJourney("someone", await users());
  const value = await issued(emu);
  const ok = await read(emu, { inventedCookie: value });
  expect(dump(ok.body)).toMatchObject({ branch: "true", username: "someone" });
  // Discriminating: an implementation that only checks a cookie is present would take true for these.
  const altered = `${value.slice(0, -3)}${value.endsWith("AAA") ? "BBB" : "AAA"}`;
  expect(dump((await read(emu, { inventedCookie: altered })).body).branch).toBe("false");
  expect(dump((await read(emu, {})).body).branch).toBe("false");
  expect(dump((await read(emu, { otherCookie: value })).body).branch).toBe("false");
  expect(await read(emu, { inventedCookie: "notajwt" })).toMatchObject({ status: 401 });
});
it("PersistentCookieDecisionNode writes the identity _id for a user the store holds", async () => {
  const emu = cookieJourney("alice", await users());
  const value = await issued(emu);
  expect(dump((await read(emu, { inventedCookie: value })).body)).toMatchObject({
    branch: "true",
    username: ALICE,
  });
});

// Stubs.
it.each([
  ["PushResultVerifierNode", {}, "TRUE"],
  ["PushWaitNode", {}, "EXITED"],
  ["PushRegistrationNode", {}, "timeoutOutcome"],
  ["product-Saml2Node", {}, "NO_ACCOUNT"],
  ["PollingWaitNode", { exitable: true }, "EXITED"],
  ["PushAuthenticationSenderNode", { mandatory: false }, "SKIPPED"],
] as const)("%s is a stub that takes its declared outcome", async (type, cfg, outcome) => {
  const emu = make(flow(type, cfg, "", [outcome]), { declaredOutcomes: { node: outcome } });
  expect(dump((await request(emu)).body).branch).toBe(outcome);
});
it("a stub with no declared outcome is missing, and its vocabulary follows its configuration", async () => {
  const cause =
    "PushAuthenticationSenderNode needs infrastructure the harness cannot model; declare its outcome in JourneyOptions.declaredOutcomes";
  expectEmulationFailure(cause);
  const input = flow("PushAuthenticationSenderNode", { mandatory: true }, "", ["SENT"]);
  expect(await request(make(input))).toMatchObject({
    status: 501,
    emulation: { missing: [cause] },
  });
  // Discriminating: SKIPPED is not an outcome of a mandatory sender, so declaring it ends the journey.
  const skipped = make(input, { declaredOutcomes: { node: "SKIPPED" } });
  expect(await request(skipped)).toMatchObject({ status: 401 });
});

// Custom (node designer) node types (docs/api/25 "Custom node types", 2026-11-25).
const DESIGNER = "designer-0123456789abcdef0123456789abcdef";
const SIZE: CustomNodeType = {
  displayName: "Size",
  outcomes: ["big", "small"],
  errorOutcome: true,
  inputs: [],
  outputs: [],
  script: "outcome = properties.limit > 2 ? 'big' : 'small';",
  properties: { limit: { type: "NUMBER", required: true, multivalued: false } },
};
function custom(def: CustomNodeType, node: Record<string, unknown>, branches: string[]) {
  const input = flow(DESIGNER, node, "", branches);
  return { ...input, customNodeTypes: { [DESIGNER]: def } };
}
it.each([
  // Discriminating: the instance's property decides, so a hard-coded outcome fails one of the two.
  [3, "big"],
  [1, "small"],
] as const)("a custom node with limit %i takes %s", async (limit, branch) => {
  const result = await request(make(custom(SIZE, { limit }, ["big", "small"])));
  expect(dump(result.body).branch).toBe(branch);
});
it("a custom node missing a required property fails the journey with 401", async () => {
  expect(await request(make(custom(SIZE, {}, ["big", "small"])))).toMatchObject({ status: 401 });
});
const RICH: CustomNodeType = {
  outcomes: ["a", "b"],
  errorOutcome: true,
  inputs: ["*"],
  outputs: ["*"],
  script:
    "var u = nodeState.get('username');" +
    "nodeState.putShared('kdemoOut', 'got:' + u + ':' + properties.label + ':' + typeof properties.flag + ':' + properties.tags.length);" +
    "if (properties.fail) { throw 'boom'; }" +
    "outcome = properties.pick;",
  properties: {
    label: { type: "STRING" },
    flag: { type: "BOOLEAN" },
    tags: { type: "STRING", multivalued: true },
    fail: { type: "BOOLEAN" },
    pick: { type: "STRING" },
  },
};
it.each([
  [
    "properties and state",
    { label: "L1", flag: true, tags: ["x", "y"], pick: "b" },
    "b",
    "got:null:L1:boolean:2",
  ],
  // Discriminating: absent properties are undefined, and an absent multivalued one is an empty array.
  ["absent properties", { pick: "a" }, "a", "got:null:undefined:undefined:0"],
  // A throw takes the error outcome and keeps what the script wrote before it.
  ["a throw", { fail: true, pick: "a" }, "Script Error", "got:null:undefined:undefined:0"],
  [
    "an outcome the type does not list",
    { pick: "zzz" },
    "Script Error",
    "got:null:undefined:undefined:0",
  ],
  ["no outcome assigned", {}, "Script Error", "got:null:undefined:undefined:0"],
] as const)("a custom node script: %s", async (_n, node, branch, out) => {
  const result = await request(make(custom(RICH, node, ["a", "b", "Script Error"])));
  expect(dump(result.body)).toMatchObject({ branch, kOut: out });
});
it("a custom node without an error outcome fails the journey with 401 when its script throws", async () => {
  const quiet = { ...RICH, errorOutcome: false };
  expect(await request(make(custom(quiet, { fail: true, pick: "a" }, ["a", "b"])))).toMatchObject({
    status: 401,
  });
});
it("a custom node type the config lacks is missing", async () => {
  const cause = `custom node type: ${DESIGNER}`;
  expectEmulationFailure(cause);
  const { customNodeTypes: _c, ...bare } = custom(SIZE, { limit: 3 }, ["big", "small"]);
  expect(await request(make(bare))).toMatchObject({ status: 501, emulation: { missing: [cause] } });
});

// Node records name the type as measured: the built-in type string, and designer-<serviceName>.
const recordsOf = (records: LogRecord[]) =>
  records
    .map((r) => r.payload as { eventName?: string; entries?: { info: Record<string, unknown> }[] })
    .filter((p) => p.eventName === "AM-NODE-LOGIN-COMPLETED")
    .map((p) => p.entries?.[0]?.info.nodeType);
it.each([
  ["AnonymousUserNode", {}, "AnonymousUserNode"],
  ["SetSuccessUrlNode", {}, "SetSuccessUrlNode"],
] as const)("the node record of %s carries its nodeType", async (type, cfg, expected) => {
  const records: LogRecord[] = [];
  const emu = make(flow(type, cfg, put({ username: "alice" }), ["outcome"], { success: true }), {
    logSink: { emit: (record: LogRecord) => records.push(record) },
  });
  expect((await request(emu)).status).toBe(200);
  expect(recordsOf(records)).toEqual(["ScriptedDecisionNode", expected]);
});
it("the node record of a custom node carries designer-<serviceName>", async () => {
  const records: LogRecord[] = [];
  const emu = make(custom(SIZE, { limit: 3 }, ["big", "small"]), {
    logSink: { emit: (record: LogRecord) => records.push(record) },
  });
  await request(emu);
  expect(recordsOf(records)).toContain(DESIGNER);
});
