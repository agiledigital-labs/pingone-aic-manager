import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { createInterface } from "node:readline/promises";
import { defaultAicIo, type CliResult } from "../aic/tenant.ts";
import { failuresDir, latestLogsPath, repoRoot } from "../paths.ts";
import {
  readFailures,
  sortNewestFirst,
  type FailureRecord,
} from "./failures.ts";

export interface TxChoice {
  id: string;
  label: string;
}

export interface ShowLogIo {
  readDump: () => Promise<FailureRecord[]>;
  print: (text: string) => void;
  error: (text: string) => void;
  selectTests: (records: FailureRecord[]) => Promise<FailureRecord[]>;
  selectIds: (record: FailureRecord, choices: TxChoice[]) => Promise<string[]>;
  aic: (args: string[]) => Promise<CliResult>;
  openEditor: (editor: string, file: string) => Promise<void>;
  writeView: (contents: string) => Promise<string>;
  env: NodeJS.ProcessEnv;
  project: string;
}

export function txChoices(record: FailureRecord): TxChoice[] {
  if (record.passIds.length <= 1) {
    return [{ id: record.stem, label: "whole chain" }];
  }
  return [
    { id: record.stem, label: "whole chain" },
    ...record.passIds.map((id, index) => ({
      id,
      label: `pass ${index + 1}`,
    })),
  ];
}

export function resolveLogsEditor(env: NodeJS.ProcessEnv): string | undefined {
  return nonEmpty(env.LOGS_EDITOR) ?? nonEmpty(env.EDITOR);
}

export function logsTxArgs(options: {
  id: string;
  tenant: string;
  project: string;
}): string[] {
  return [
    "--no-prompt",
    "--project",
    options.project,
    "logs",
    "tx",
    options.id,
    "--tenant",
    options.tenant,
    "--wait",
  ];
}

export function formatFailureList(records: readonly FailureRecord[]): string {
  return records
    .map((record, index) => {
      const n = String(index + 1).padStart(String(records.length).length, " ");
      return `${n}. ${record.timestamp}  ${record.testName}  (${record.tenant})`;
    })
    .join("\n");
}

export function formatChoices(choices: readonly TxChoice[]): string {
  return choices
    .map((choice, index) => `${index + 1}. ${choice.id}  (${choice.label})`)
    .join("\n");
}

/**
 * Empty input selects the first entry (newest test / the stem). `all` selects
 * every entry. Invalid input returns undefined so the caller can re-prompt.
 */
export function parseSelection(
  input: string,
  count: number,
  empty: "first" | "all" = "first"
): number[] | undefined {
  if (count < 1) {
    return [];
  }
  const trimmed = input.trim().toLowerCase();
  if (trimmed === "") {
    return empty === "all" ? range(0, count) : [0];
  }
  if (trimmed === "all") {
    return range(0, count);
  }
  const picked: number[] = [];
  const seen = new Set<number>();
  for (const part of trimmed.split(",")) {
    const piece = part.trim();
    const span = /^(\d+)-(\d+)$/.exec(piece);
    if (span !== null) {
      const start = Number(span[1]);
      const end = Number(span[2]);
      if (!Number.isInteger(start) || !Number.isInteger(end) || start < 1 || end > count || start > end) {
        return undefined;
      }
      for (let index = start - 1; index < end; index += 1) {
        if (!seen.has(index)) {
          seen.add(index);
          picked.push(index);
        }
      }
      continue;
    }
    const n = Number(piece);
    if (!Number.isInteger(n) || n < 1 || n > count) {
      return undefined;
    }
    const index = n - 1;
    if (!seen.has(index)) {
      seen.add(index);
      picked.push(index);
    }
  }
  return picked.length === 0 ? undefined : picked;
}

export async function runShowLog(io: ShowLogIo): Promise<number> {
  const records = sortNewestFirst(await io.readDump());
  if (records.length === 0) {
    io.print("no failed tests recorded. Run a test that hits the AIC lane first.");
    return 0;
  }
  io.print(formatFailureList(records));
  const onlyRecord = records[0];
  const selected =
    records.length === 1 && onlyRecord !== undefined
      ? [onlyRecord]
      : await io.selectTests(records);
  if (selected.length === 0) {
    io.print("nothing selected");
    return 0;
  }
  const chunks: string[] = [];
  for (const record of selected) {
    const choices = txChoices(record);
    const onlyChoice = choices[0];
    const ids =
      choices.length === 1 && onlyChoice !== undefined
        ? [onlyChoice.id]
        : await io.selectIds(record, choices);
    for (const id of ids) {
      const result = await io.aic(
        logsTxArgs({ id, tenant: record.tenant, project: io.project })
      );
      if (result.status !== 0) {
        io.error(formatAicFailure(result));
        return result.status;
      }
      const body = result.stdout.trimEnd();
      chunks.push(
        ids.length === 1 && selected.length === 1
          ? body
          : `=== ${id} (${record.testName}) ===\n${body}`
      );
    }
  }
  const text = `${chunks.join("\n\n")}\n`;
  const editor = resolveLogsEditor(io.env);
  if (editor === undefined) {
    io.print(text);
    return 0;
  }
  const viewPath = await io.writeView(text);
  await io.openEditor(editor, viewPath);
  return 0;
}

export function createDefaultIo(
  options: { env?: NodeJS.ProcessEnv; project?: string } = {}
): ShowLogIo {
  const env = options.env ?? process.env;
  const project = options.project ?? repoRoot;
  const aicIo = defaultAicIo(project);
  return {
    env,
    project,
    readDump: () => readFailures(),
    print: (text) => {
      process.stdout.write(`${text}\n`);
    },
    error: (text) => {
      process.stderr.write(`${text}\n`);
    },
    async selectTests(records) {
      process.stdout.write("\nSelect test (comma-separated, all) [1]: ");
      const picked = await readSelection(records.length);
      return picked.flatMap((index) => {
        const record = records[index];
        return record === undefined ? [] : [record];
      });
    },
    async selectIds(record, choices) {
      process.stdout.write(`\n${record.testName}\n${formatChoices(choices)}\n`);
      process.stdout.write("Select transaction id(s) [1 = whole chain]: ");
      const picked = await readSelection(choices.length);
      return picked.flatMap((index) => {
        const id = choices[index]?.id;
        return id === undefined ? [] : [id];
      });
    },
    aic: (args) => aicIo.aic(args),
    openEditor,
    async writeView(contents) {
      await mkdir(failuresDir, { recursive: true });
      // 0600: this holds a live tenant's log bodies — hostnames, client ips,
      // request headers. The directory is gitignored, but the file should not
      // be world-readable either.
      await writeFile(latestLogsPath, contents, { encoding: "utf8", mode: 0o600 });
      return latestLogsPath;
    },
  };
}

async function readSelection(count: number): Promise<number[]> {
  if (process.stdin.isTTY !== true) {
    throw new Error(
      "show-log: need a terminal to select among multiple entries. Re-run from a tty."
    );
  }
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  try {
    const answer = await rl.question("");
    const picked = parseSelection(answer, count);
    if (picked === undefined) {
      throw new Error(`show-log: could not parse ${JSON.stringify(answer)}`);
    }
    return picked;
  } finally {
    rl.close();
  }
}

/**
 * `shell: true` is deliberate — `$EDITOR` is routinely a command line
 * (`code -w`, `vim -p`), not a bare executable. But an args array WITH a shell
 * is what Node deprecated in DEP0190, because the shell re-splits what was
 * already separated. So build the one command string the shell will actually
 * run, and quote the path ourselves.
 */
function openEditor(editor: string, file: string): Promise<void> {
  const command = `${editor} ${shellQuote(file)}`;
  return new Promise((resolve, reject) => {
    const child = spawn(command, { stdio: "inherit", shell: true });
    child.on("error", reject);
    child.on("exit", (code) => {
      if (code === 0 || code === null) {
        resolve();
        return;
      }
      reject(new Error(`${editor} exited ${code}`));
    });
  });
}

function formatAicFailure(result: CliResult): string {
  const body = (result.stderr || result.stdout).trim();
  const hint = "Unlock the agent (`aic login`) if it is locked.";
  if (body.length === 0) {
    return `aic logs tx failed (status ${result.status}). ${hint}`;
  }
  if (/locked|aic login|session login/i.test(body)) {
    return body;
  }
  return `${body}\n${hint}`;
}

function nonEmpty(value: string | undefined): string | undefined {
  if (value === undefined) {
    return undefined;
  }
  const trimmed = value.trim();
  return trimmed.length === 0 ? undefined : trimmed;
}

function range(start: number, end: number): number[] {
  const out: number[] = [];
  for (let index = start; index < end; index += 1) {
    out.push(index);
  }
  return out;
}

/** Single-quote for a POSIX shell, closing and reopening around any quote. */
function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}
