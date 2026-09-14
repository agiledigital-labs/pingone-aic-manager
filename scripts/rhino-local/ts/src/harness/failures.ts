import { appendFile, mkdir, readFile } from "node:fs/promises";
import { dirname } from "node:path";
import type { AicTrace } from "../aic/trace.ts";
import { isPlainObject } from "../case/util.ts";
import { failuresPath } from "../paths.ts";

export interface FailureRecord {
  testName: string;
  stem: string;
  passIds: string[];
  file: string;
  suite: string;
  timestamp: string;
  tenant: string;
}

/**
 * A local-only failure has no transaction id, and must not produce a record
 * that would send `show-log` off to fetch nothing.
 */
export function failureRecordFor(input: {
  trace: AicTrace | undefined;
  testName: string;
  file: string;
  suite: string;
  timestamp: string;
}): FailureRecord | undefined {
  const trace = input.trace;
  if (trace === undefined || trace.stem.length === 0 || trace.passIds.length === 0) {
    return undefined;
  }
  return {
    testName: input.testName,
    stem: trace.stem,
    passIds: [...trace.passIds],
    file: input.file,
    suite: input.suite,
    timestamp: input.timestamp,
    tenant: trace.tenantName,
  };
}

export async function appendFailure(
  record: FailureRecord,
  path = failuresPath
): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await appendFile(path, `${JSON.stringify(record)}\n`, { encoding: "utf8" });
}

export async function recordFailureIfAny(
  input: Parameters<typeof failureRecordFor>[0],
  path = failuresPath
): Promise<FailureRecord | undefined> {
  const record = failureRecordFor(input);
  if (record === undefined) {
    return undefined;
  }
  await appendFailure(record, path);
  return record;
}

export async function readFailures(path = failuresPath): Promise<FailureRecord[]> {
  let text: string;
  try {
    text = await readFile(path, "utf8");
  } catch (error) {
    if (isEnoent(error)) {
      return [];
    }
    throw error;
  }
  return parseFailures(text);
}

export function parseFailures(text: string): FailureRecord[] {
  const records: FailureRecord[] = [];
  for (const line of text.split(/\r?\n/)) {
    if (line.trim() === "") {
      continue;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(line);
    } catch {
      continue;
    }
    const record = asFailureRecord(parsed);
    if (record !== undefined) {
      records.push(record);
    }
  }
  return records;
}

export function sortNewestFirst(records: readonly FailureRecord[]): FailureRecord[] {
  return records
    .map((record, index) => ({ record, index }))
    .sort((a, b) => {
      const time = b.record.timestamp.localeCompare(a.record.timestamp);
      return time !== 0 ? time : b.index - a.index;
    })
    .map((row) => row.record);
}

function asFailureRecord(value: unknown): FailureRecord | undefined {
  if (!isPlainObject(value)) {
    return undefined;
  }
  if (
    typeof value.testName !== "string" ||
    typeof value.stem !== "string" ||
    value.stem.length === 0 ||
    !isStringArray(value.passIds) ||
    value.passIds.length === 0 ||
    typeof value.file !== "string" ||
    typeof value.suite !== "string" ||
    typeof value.timestamp !== "string" ||
    typeof value.tenant !== "string"
  ) {
    return undefined;
  }
  return {
    testName: value.testName,
    stem: value.stem,
    passIds: value.passIds,
    file: value.file,
    suite: value.suite,
    timestamp: value.timestamp,
    tenant: value.tenant,
  };
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function isEnoent(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === "ENOENT"
  );
}
