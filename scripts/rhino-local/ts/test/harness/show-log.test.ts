import { describe, expect, it } from "vitest";
import type { CliResult } from "../../src/aic/tenant.ts";
import type { FailureRecord } from "../../src/harness/failures.ts";
import {
  formatFailureList,
  logsTxArgs,
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
    expect(fake.aicCalls).toEqual([
      logsTxArgs({ id: "stem-one", tenant: "sandbox", project: "/tmp/project" }),
    ]);
    expect(fake.aicCalls[0]?.[0]).toBe("--no-prompt");
    expect(fake.aicCalls[0]?.includes("range")).toBe(false);
    expect(fake.aicCalls[0]?.includes("tx")).toBe(true);
  });

  it("with several ids offers a multi-select", async () => {
    const fake = trackingIo({
      records: [MANY],
      pickIds: ["stem-many-01", "stem-many-10"],
    });
    await runShowLog(fake.io);
    expect(fake.idPrompts).toBe(1);
    expect(fake.aicCalls.map((args) => args[args.indexOf("tx") + 1])).toEqual([
      "stem-many-01",
      "stem-many-10",
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

  it("neither editor set prints to stdout", async () => {
    const fake = trackingIo({ records: [ONE], env: {} });
    await runShowLog(fake.io);
    expect(fake.opened).toEqual([]);
    expect(fake.printed.some((line) => line.includes("[events]"))).toBe(true);
  });

  it("a locked daemon fails fast with the aic message, not a hang", async () => {
    const fake = trackingIo({
      records: [ONE],
      aicResult: {
        status: 3,
        stdout: "",
        stderr: "agent is locked — run `aic session login`",
      },
    });
    const status = await runShowLog(fake.io);
    expect(status).toBe(3);
    expect(fake.errors.some((line) => /locked/.test(line))).toBe(true);
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

describe("logsTxArgs", () => {
  it("is always `aic logs tx` with --no-prompt, never a range query", () => {
    const args = logsTxArgs({ id: "stem", tenant: "sandbox", project: "/repo" });
    expect(args[0]).toBe("--no-prompt");
    expect(args).toContain("tx");
    expect(args).toContain("stem");
    expect(args).not.toContain("range");
    expect(args).not.toContain("query");
  });
});

describe("formatFailureList", () => {
  it("lists newest-first input as numbered rows with timestamps", () => {
    const listed = formatFailureList([MANY, ONE]);
    expect(listed).toContain("1. 2026-09-14T13:00:00.000Z  suite > many passes");
    expect(listed).toContain("2. 2026-09-14T12:00:00.000Z  suite > one pass");
  });
});

function trackingIo(options: {
  records: FailureRecord[];
  env?: NodeJS.ProcessEnv;
  pickIds?: string[];
  aicResult?: CliResult;
}): {
  io: ShowLogIo;
  aicCalls: string[][];
  opened: string[];
  printed: string[];
  errors: string[];
  idPrompts: number;
  testPrompts: number;
} {
  const aicCalls: string[][] = [];
  const opened: string[] = [];
  const printed: string[] = [];
  const errors: string[] = [];
  const state = { idPrompts: 0, testPrompts: 0 };
  const io: ShowLogIo = {
    env: options.env ?? {},
    project: "/tmp/project",
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
    aic: (args) => {
      aicCalls.push(args);
      return Promise.resolve(
        options.aicResult ?? { status: 0, stdout: "[events]\n", stderr: "" }
      );
    },
    openEditor: (editor, file) => {
      opened.push(`${editor}:${file}`);
      return Promise.resolve();
    },
    writeView: (contents) => {
      expect(contents.length).toBeGreaterThan(0);
      return Promise.resolve("/tmp/view.json");
    },
  };
  return {
    io,
    aicCalls,
    opened,
    printed,
    errors,
    get idPrompts() {
      return state.idPrompts;
    },
    get testPrompts() {
      return state.testPrompts;
    },
  };
}
