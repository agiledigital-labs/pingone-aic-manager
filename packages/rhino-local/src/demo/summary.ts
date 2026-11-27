/**
 * What `aic-emulator-demo` says about a run: the periodic status line, the final report, and the
 * exit status. A journey the engine answered 400 or 501 is a demo that did not show what it
 * claims (a 501 is emulator `missing` behaviour), so it is printed and fails the run.
 */
import type { Report } from "./driver.ts";
import { formatRfc3339Ns } from "../logs/time.ts";

/** Statuses that make the run fail: the engine refused the request or the emulator lacks behaviour. */
const FAILING = ["400", "501"];

const seconds = (ns: bigint): number => Number(ns / 1_000_000n) / 1000;

export function statusLine(report: Report, nowNs: bigint): string {
  const j = report.journeys;
  return (
    `${formatRfc3339Ns(nowNs)} journeys ${j.started} (ok ${j.succeeded}, failed ${j.failed}, ` +
    `abandoned ${j.abandoned}, errored ${j.errored}) records ${report.records.total} ` +
    `max lag ${seconds(report.lag.overall.maxNs)} s`
  );
}

export function finalReport(report: Report): string {
  const j = report.journeys;
  const lines = [
    `run ${formatRfc3339Ns(report.fromNs)} to ${formatRfc3339Ns(report.toNs)} (virtual), ${Math.round(report.realMs)} ms real`,
    `journeys ${j.started}: ok ${j.succeeded}, failed ${j.failed}, abandoned ${j.abandoned}, ` +
      `broken by endpoint ${j.brokenByEndpoint}, errored ${j.errored}`,
    `requests ${report.requests}, records ${report.records.total}`,
  ];
  for (const [status, count] of Object.entries(j.erroredByStatus).sort())
    lines.push(`errored with HTTP ${status}: ${count}`);
  for (const [message, count] of Object.entries(report.engineErrors).sort())
    lines.push(`  ${count} x ${message}`);
  for (const [url, count] of Object.entries(report.unmockedHttp).sort())
    lines.push(`unmocked HTTP ${count} x ${url}`);
  if (report.behind) lines.push(`behind the clock: max lag ${seconds(report.lag.overall.maxNs)} s`);
  return lines.join("\n");
}

export function exitCode(report: Report): number {
  return FAILING.some((status) => (report.journeys.erroredByStatus[status] ?? 0) > 0) ? 1 : 0;
}
