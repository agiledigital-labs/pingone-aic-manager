/**
 * One log record as AIC's `GET /monitoring/logs` returns it, and the seam
 * between what produces records (the journey engine, the traffic driver) and
 * what serves them (the log store behind the emulated monitoring API).
 *
 * Shape per docs/api/08-logs.md: `payload` is JSON for structured streams and a
 * string for text streams. `timestamp` is RFC 3339 in the emulator's (possibly
 * virtual) clock.
 */
export type LogSource =
  | "am-access"
  | "am-activity"
  | "am-authentication"
  | "am-config"
  | "am-core"
  | "idm-access"
  | "idm-activity"
  | "idm-authentication"
  | "idm-config"
  | "idm-core"
  | "idm-sync";

export type LogRecord = {
  readonly source: LogSource;
  readonly timestamp: string;
  readonly type: "application/json" | "text/plain";
  readonly payload: Readonly<Record<string, unknown>> | string;
};

/** Where producers put records. Implementations must not reorder one producer's records. */
export interface LogSink {
  emit(record: LogRecord): void;
}
