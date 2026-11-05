/**
 * The run's harness-fault tally (E19): for each stable fault id, how many cases
 * hit it and in which test files. Built in the reporter (main process) from the
 * emulation audits every worker sends, so it covers every file, worker and
 * pooled JVM. A case counts once per id however often it hit it, and counts
 * even when the script caught the refusal — the fault reached the audit either
 * way. Faults are recorded per job in the JVM (java/HarnessFaults.java) and
 * attributed to the test that started the job, so one case's fault cannot
 * appear under another's.
 *
 * Missing journey/IDM behaviour that has no id yet is listed separately under
 * its cause text, which is not stable across releases.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import type { EmulationAudit } from "../emulation.ts";
import { ensureStateSubdir, stateDir } from "../project.ts";

/** Overrides {@link faultTallyPath}. */
export const FAULT_TALLY_ENV = "AIC_SCRIPT_TESTER_FAULT_TALLY";

export interface FaultTallyRow {
  /** A stable fault id, or for `unidentified`, the cause text. */
  id: string;
  /** Distinct cases (tests, or a suite's hooks) that hit it. */
  cases: number;
  /**
   * Of those, the cases it failed. The rest declared it on purpose
   * (`expectHarnessFault` / `expectEmulationFailure`), as a test of the
   * harness itself does; a client suite's tally is normally all failures.
   */
  failed: number;
  /** Test files those cases are in, relative to the Vitest root, sorted. */
  files: string[];
  /** The first message seen for it, for a reader without the source. */
  message: string;
}

export interface FaultTally {
  version: 1;
  /** Rows sorted by `cases` descending, then id. */
  faults: FaultTallyRow[];
  /** Missing behaviour with no fault id (journey and IDM emulator sites). */
  unidentified: FaultTallyRow[];
}

interface Acc { cases: Set<string>; failed: Set<string>; files: Set<string>; message: string }

/** `root`, when given, makes file paths under it relative (the reporter passes Vitest's root). */
export function tallyFaults(source: Iterable<EmulationAudit>, root?: string): FaultTally {
  const audits = [...source];
  const shown = (file: string): string => {
    if (root === undefined) return file;
    const path = relative(root, file);
    return path === "" || path.startsWith("..") || isAbsolute(path) ? file : path;
  };
  const byId = new Map<string, Acc>();
  const byCause = new Map<string, Acc>();
  const add = (into: Map<string, Acc>, key: string, kase: string, file: string | undefined, message: string, failed: boolean): void => {
    let acc = into.get(key);
    if (acc === undefined) into.set(key, (acc = { cases: new Set(), failed: new Set(), files: new Set(), message }));
    acc.cases.add(kase);
    if (failed) acc.failed.add(kase);
    if (file !== undefined) acc.files.add(shown(file));
  };
  // An id'd fault raised inside a host call is recorded twice: once by the host
  // callback with its id, and again by the job's own response, which carries
  // the cause ("<id>: <message>") but not the id. Same case, same cause: one fault.
  const identified = new Set<string>();
  for (const audit of audits) {
    for (const entry of audit.failures) {
      const kase = entry.owner?.id ?? `unowned:${entry.file ?? entry.owner?.file ?? ""}`;
      for (const fault of entry.faults ?? []) identified.add(`${kase}\0${fault.id}: ${fault.message}`);
    }
  }
  for (const audit of audits) {
    for (const entry of audit.failures) {
      const file = entry.file ?? entry.owner?.file;
      // Owner ids are random UUIDs, unique across workers; an unowned entry is
      // one case per file.
      const kase = entry.owner?.id ?? `unowned:${file ?? ""}`;
      if (entry.faults?.length) {
        // A fault's cause is "<id>: <message>"; it is in `causes` only when
        // the case did not declare it.
        for (const fault of entry.faults) {
          add(byId, fault.id, kase, file, fault.message, entry.causes.some((cause) => cause.startsWith(`${fault.id}: `)));
        }
      } else if (entry.kind === "missing") {
        for (const cause of entry.observed ?? []) {
          if (!identified.has(`${kase}\0${cause}`)) add(byCause, cause, kase, file, cause, entry.causes.includes(cause));
        }
      }
    }
  }
  return { version: 1, faults: rows(byId), unidentified: rows(byCause) };
}

function rows(from: Map<string, Acc>): FaultTallyRow[] {
  return [...from]
    .map(([id, acc]) => ({ id, cases: acc.cases.size, failed: acc.failed.size, files: [...acc.files].sort(), message: acc.message }))
    .sort((a, b) => b.cases - a.cases || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

/** `AIC_SCRIPT_TESTER_FAULT_TALLY`, else `<state dir>/harness-faults/tally.json`. */
export function faultTallyPath(env: NodeJS.ProcessEnv = process.env): string {
  const configured = env[FAULT_TALLY_ENV];
  if (configured !== undefined && configured !== "") return resolve(configured);
  return join(stateDir(env), "harness-faults", "tally.json");
}

/**
 * Writes the tally and returns where. Always written, even empty, so a reader
 * can tell "no faults" from "no run". The default location goes through the
 * state directory's self-ignoring guard, since fault messages can name a
 * tenant's vocabulary.
 */
export function writeFaultTally(tally: FaultTally, env: NodeJS.ProcessEnv = process.env): string {
  const path = faultTallyPath(env);
  const configured = env[FAULT_TALLY_ENV];
  if (configured !== undefined && configured !== "") mkdirSync(dirname(path), { recursive: true });
  else ensureStateSubdir(stateDir(env), "harness-faults");
  writeFileSync(path, `${JSON.stringify(tally, null, 2)}\n`, { encoding: "utf8", mode: 0o600 });
  return path;
}

/** The end-of-run summary, one line per id; empty when the run hit none. */
export function formatFaultTally(tally: FaultTally, path: string): string {
  if (tally.faults.length === 0 && tally.unidentified.length === 0) return "";
  const lines = ["", "rhino-local harness faults (behaviour this harness does not emulate; a case that hits one fails unless it declared it):"];
  const declared = (row: FaultTallyRow): string => (row.failed === row.cases ? "" : `, ${row.cases - row.failed} declared`);
  for (const row of tally.faults) {
    lines.push(`  ${String(row.cases).padStart(4)} case${row.cases === 1 ? " " : "s"}  ${row.id}  (${row.files.length} file${row.files.length === 1 ? "" : "s"}${declared(row)})`);
  }
  if (tally.unidentified.length > 0) {
    lines.push("  missing behaviour without a fault id:");
    for (const row of tally.unidentified) {
      const note = declared(row);
      lines.push(`  ${String(row.cases).padStart(4)} case${row.cases === 1 ? " " : "s"}  ${row.id}${note === "" ? "" : `  (${note.slice(2)})`}`);
    }
  }
  lines.push(`  tally: ${path}`, "");
  return lines.join("\n");
}
