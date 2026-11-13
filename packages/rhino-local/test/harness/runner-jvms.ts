/**
 * Count the runner JVMs a process tree starts, from outside it: `/proc` is
 * sampled and every distinct `java … Runner` descendant of the root pid is
 * recorded. A runner JVM lives far longer than one sample, so none is missed.
 * Used by bench/run.ts and by the two-file pool test, which must prove the
 * lease itself was pooled rather than infer it from a bare borrow.
 *
 * Linux only: `available` is false where there is no `/proc`.
 */
import { existsSync, readdirSync, readFileSync } from "node:fs";

export const procAvailable = existsSync("/proc/self/stat");

function processTable(): Map<number, { ppid: number; comm: string }> {
  const table = new Map<number, { ppid: number; comm: string }>();
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const stat = readFileSync(`/proc/${entry}/stat`, "utf8");
      const close = stat.lastIndexOf(")");
      const comm = stat.slice(stat.indexOf("(") + 1, close);
      const ppid = Number(stat.slice(close + 2).split(" ")[1]);
      table.set(Number(entry), { ppid, comm });
    } catch {
      // exited between readdir and read
    }
  }
  return table;
}

function sample(rootPid: number, seen: Set<number>): void {
  const table = processTable();
  const isDescendant = (pid: number): boolean => {
    for (let p = pid, hops = 0; p > 1 && hops < 64; hops += 1) {
      if (p === rootPid) return true;
      const row = table.get(p);
      if (row === undefined) return false;
      p = row.ppid;
    }
    return false;
  };
  for (const [pid, row] of table) {
    if (!row.comm.includes("java") || seen.has(pid) || !isDescendant(pid)) continue;
    try {
      // The runner itself, not javac or a `java -version` probe.
      const argv = readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0");
      if (argv.includes("Runner")) seen.add(pid);
    } catch {
      // exited
    }
  }
}

/** Start sampling under `rootPid`; `stop()` stops sampling and returns the distinct runner pids seen so far (no final sample: runner JVMs outlive many intervals). */
export function watchRunnerJvms(rootPid: number, everyMs = 25): { stop: () => Set<number> } {
  const seen = new Set<number>();
  const timer = setInterval(() => sample(rootPid, seen), everyMs);
  return {
    stop: () => {
      clearInterval(timer);
      return seen;
    },
  };
}
