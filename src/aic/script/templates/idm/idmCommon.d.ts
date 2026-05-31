interface JavaString {
  new (value: StringLike): JavaString;

  includes(value: StringLike): boolean;

  split(separator: StringLike): JavaArray<JavaString>;
}

type StringLike = string | JavaString;

interface JavaArray<T = JavaString> {
  [index: number]: T | null | undefined;

  length: number;

  includes(value: T): boolean;

  asList(): any[];
}

interface JavaMap<Key = JavaString, Value = JavaString> {
  get(key: Key): Value | null;
}

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
  query: (path: string, { _queryFilter: string }) => any;
  create: (
    path: string,
    newResourceId: string,
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

type IdmRequest = {
  method:
    | "create"
    | "read"
    | "update"
    | "delete"
    | "patch"
    | "query"
    | "action";
  resourcePath: string;
  newResourceId?: string;
  revision?: string;
  patchOperations: any;
  content: any;
  action?: string;
  pagedResults: any;
  pagedResultsCookie?: string;
  pagedResultsOffset?: number;
  pageSize?: number;
  queryFilter?: string;
  queryId?: string;
  additionalParameters: Record<string, string>;
};

declare const request: IdmRequest;

type Context = {
  current: {
    parent: {
      parent: {
        parent: {
          headers: Record<string, string[]>;
          method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
        };
      };
    };
  };
};

declare const context: Context;
