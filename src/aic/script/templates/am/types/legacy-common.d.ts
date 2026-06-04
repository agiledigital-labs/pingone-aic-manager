// Classic AM `logger` shape, shared by the legacy scripted-decision engine and
// the other (mostly unmigrated) AM script contexts. Layered on rhino + common.
//
// Runtime-verified on the legacy engine (2026-06-04): `logger` is the classic
// Debug object — error/message/warning plus the *Enabled guards. The slf4j-style
// trace/debug/info/warn are ABSENT here (those are the next-gen shape; see
// nextgen-common.d.ts).
interface Logger {
  error(message: StringLike): void;
  message(message: StringLike): void;
  warning(message: StringLike): void;
  errorEnabled(): boolean;
  messageEnabled(): boolean;
  warningEnabled(): boolean;
}
declare const logger: Logger;

// systemEnv is a legacy-only binding (present on the legacy probe; absent from
// every next-gen context's binding metadata).
interface SystemEnv {
  getProperty: (key: StringLike) => JavaString | null;
}
declare const systemEnv: SystemEnv;
