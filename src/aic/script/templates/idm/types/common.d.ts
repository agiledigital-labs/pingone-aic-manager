// Common IDM bindings shared by all IDM script families (endpoint, schedule).
// Layered on top of rhino-1.7.14.d.ts.
//
// IDM binding availability is inferred from the prior template + docs; not yet
// runtime-probed (no local IDM sample corpus). See docs/api/12 IDM open items.

type LogFunction = (message: StringLike, ...args: any[]) => void;
interface Logger {
  trace: LogFunction;
  debug: LogFunction;
  info: LogFunction;
  warn: LogFunction;
  error: LogFunction;
}
declare const logger: Logger;

type Patch =
  | {
      operation: "add" | "replace";
      field: string;
      value: string;
    }
  | {
      operation: "remove";
      field: string;
    };

declare const openidm: {
  read: (
    path: string,
    params?: Record<string, string> | null,
    fields?: string[]
  ) => any;
  query: (path: string, params: { _queryFilter: string }) => any;
  create: (
    path: string,
    newResourceId: string | null,
    content: Record<string, any> | null,
    params?: Record<string, string> | null
  ) => any;
  update: (
    path: string,
    revision: string | null,
    content: Record<string, any> | null,
    params?: Record<string, string> | null
  ) => any;
  patch: (path: string, revision: string | null, patch: Patch[]) => any;
};
