import { generateKeyPairSync } from "node:crypto";
import { describe, expect, it } from "vitest";
import { TENANT_ENV } from "../../src/aic/provider.ts";
import type { FailureRecord } from "../../src/harness/failures.ts";
import {
  createDefaultIo,
  formatFailureList,
  parseSelection,
  resolveLogsEditor,
  runShowLog,
  txChoices,
  type ShowLogIo,
} from "../../src/harness/show-log.ts";

const ONE: FailureRecord = {
  testName: "suite > one pass",
  stem: "stem-one",
  passIds: ["stem-one-01"],
  file: "test/a.test.ts",
  suite: "suite",
  timestamp: "2026-09-14T12:00:00.000Z",
  tenant: "sandbox",
};

const MANY: FailureRecord = {
  testName: "suite > many passes",
  stem: "stem-many",
  passIds: ["stem-many-01", "stem-many-02", "stem-many-10"],
  file: "test/b.test.ts",
  suite: "suite",
  timestamp: "2026-09-14T13:00:00.000Z",
  tenant: "sandbox",
};

describe("txChoices", () => {
  it("a single pass is one id — the stem — so show-log can skip the prompt", () => {
    expect(txChoices(ONE)).toEqual([{ id: "stem-one", label: "whole chain" }]);
  });

  it("several passes offer the stem plus each pass", () => {
    expect(txChoices(MANY).map((choice) => choice.id)).toEqual([
      "stem-many",
      "stem-many-01",
      "stem-many-02",
      "stem-many-10",
    ]);
  });
});

describe("runShowLog", () => {
  it("with exactly one id skips the prompt and fetches the stem", async () => {
    const fake = trackingIo({ records: [ONE] });
    const status = await runShowLog(fake.io);
    expect(status).toBe(0);
    expect(fake.idPrompts).toBe(0);
    expect(fake.testPrompts).toBe(0);
    expect(fake.reads).toEqual([["sandbox", "stem-one"]]);
    expect(fake.written.join("\n")).toContain(JSON.stringify([{ event: "ok" }], null, 2));
  });

  it("with several ids offers a multi-select", async () => {
    const fake = trackingIo({
      records: [MANY],
      pickIds: ["stem-many-01", "stem-many-10"],
    });
    await runShowLog(fake.io);
    expect(fake.idPrompts).toBe(1);
    expect(fake.reads).toEqual([
      ["sandbox", "stem-many-01"],
      ["sandbox", "stem-many-10"],
    ]);
  });

  it("LOGS_EDITOR wins over EDITOR", async () => {
    const fake = trackingIo({
      records: [ONE],
      env: { LOGS_EDITOR: "view", EDITOR: "vim" },
    });
    await runShowLog(fake.io);
    expect(fake.opened).toEqual(["view:/tmp/view.json"]);
    expect(fake.printed.join("\n")).not.toContain("[events]");
  });

  it("EDITOR is used when LOGS_EDITOR is unset", async () => {
    const fake = trackingIo({
      records: [ONE],
      env: { EDITOR: "vim" },
    });
    await runShowLog(fake.io);
    expect(fake.opened).toEqual(["vim:/tmp/view.json"]);
  });

  // The bodies are tenant data: with no editor (CI, typically) they go to the
  // protected view and only its path reaches stdout.
  it("neither editor set writes the view and prints only its path", async () => {
    const fake = trackingIo({ records: [ONE], env: {} });
    await runShowLog(fake.io);
    expect(fake.opened).toEqual([]);
    expect(fake.written).toHaveLength(1);
    expect(fake.printed.join("\n")).toContain("wrote /tmp/view.json");
    expect(fake.printed.join("\n")).not.toContain('"event"');
  });

  it("--stdout prints the logs and writes no view", async () => {
    const fake = trackingIo({ records: [ONE], env: { EDITOR: "vim" } });
    await runShowLog(fake.io, { stdout: true });
    expect(fake.opened).toEqual([]);
    expect(fake.written).toEqual([]);
    expect(fake.printed.join("\n")).toContain(JSON.stringify([{ event: "ok" }], null, 2));
  });

  it("a log read rejection prints its message and returns 1", async () => {
    const fake = trackingIo({
      records: [ONE],
      readError: "log service unavailable",
    });
    const status = await runShowLog(fake.io);
    expect(status).toBe(1);
    expect(fake.errors).toEqual(["log service unavailable"]);
    expect(fake.opened).toEqual([]);
  });
});

describe("parseSelection", () => {
  it("empty selects the first entry, all selects every entry", () => {
    expect(parseSelection("", 3)).toEqual([0]);
    expect(parseSelection("all", 3)).toEqual([0, 1, 2]);
    expect(parseSelection("1,3", 3)).toEqual([0, 2]);
    expect(parseSelection("2-3", 3)).toEqual([1, 2]);
    expect(parseSelection("9", 3)).toBeUndefined();
  });
});

describe("resolveLogsEditor", () => {
  it("LOGS_EDITOR wins, empty strings fall through, neither is unset", () => {
    expect(resolveLogsEditor({ LOGS_EDITOR: "view", EDITOR: "vim" })).toBe("view");
    expect(resolveLogsEditor({ LOGS_EDITOR: "  ", EDITOR: "vim" })).toBe("vim");
    expect(resolveLogsEditor({ EDITOR: "" })).toBeUndefined();
    expect(resolveLogsEditor({})).toBeUndefined();
  });
});

describe("createDefaultIo with a configured provider", () => {
  const jwk = JSON.stringify(
    generateKeyPairSync("rsa", { modulusLength: 2048 }).privateKey.export({ format: "jwk" })
  );
  const env = (extra: NodeJS.ProcessEnv = {}): NodeJS.ProcessEnv => ({
    [TENANT_ENV.url]: "https://tenant.example.com",
    [TENANT_ENV.serviceAccountId]: "00000000-0000-0000-0000-000000000001",
    [TENANT_ENV.jwk]: jwk,
    ...extra,
  });

  it("refuses a failure recorded on another tenant", async () => {
    const io = createDefaultIo({ env: env() });
    await expect(io.readTransaction("sandbox", "stem")).rejects.toThrow(
      /ran on a different tenant from the one the configured tenant provider serves/
    );
  });

  it("refuses a provider without log keys, naming the variables", async () => {
    const io = createDefaultIo({ env: env() });
    await expect(io.readTransaction("tenant.example.com", "stem")).rejects.toThrow(
      /RHINO_LOCAL_LOG_KEY_ID/
    );
  });
});

describe("formatFailureList", () => {
  it("lists newest-first input as numbered rows with timestamps", () => {
    const listed = formatFailureList([MANY, ONE]);
    expect(listed).toContain("1. 2026-09-14T13:00:00.000Z  suite > many passes");
    expect(listed).toContain("2. 2026-09-14T12:00:00.000Z  suite > one pass");
    expect(listed).not.toContain("sandbox");
  });
});

function trackingIo(options: {
  records: FailureRecord[];
  env?: NodeJS.ProcessEnv;
  pickIds?: string[];
  readError?: string;
}): {
  io: ShowLogIo;
  reads: [string, string][];
  opened: string[];
  printed: string[];
  written: string[];
  errors: string[];
  idPrompts: number;
  testPrompts: number;
} {
  const reads: [string, string][] = [];
  const opened: string[] = [];
  const printed: string[] = [];
  const written: string[] = [];
  const errors: string[] = [];
  const state = { idPrompts: 0, testPrompts: 0 };
  const io: ShowLogIo = {
    env: options.env ?? {},
    readDump: () => Promise.resolve(options.records),
    print: (text) => {
      printed.push(text);
    },
    error: (text) => {
      errors.push(text);
    },
    selectTests: (records) => {
      state.testPrompts += 1;
      return Promise.resolve(records);
    },
    selectIds: (_record, choices) => {
      state.idPrompts += 1;
      return Promise.resolve(options.pickIds ?? choices.map((choice) => choice.id));
    },
    readTransaction: (tenant, id) => {
      reads.push([tenant, id]);
      return options.readError
        ? Promise.reject(new Error(options.readError))
        : Promise.resolve([{ event: "ok" }]);
    },
    openEditor: (editor, file) => {
      opened.push(`${editor}:${file}`);
      return Promise.resolve();
    },
    writeView: (contents) => {
      expect(contents.length).toBeGreaterThan(0);
      written.push(contents);
      return Promise.resolve("/tmp/view.json");
    },
  };
  return {
    io,
    reads,
    opened,
    printed,
    written,
    errors,
    get idPrompts() {
      return state.idPrompts;
    },
    get testPrompts() {
      return state.testPrompts;
    },
  };
}
