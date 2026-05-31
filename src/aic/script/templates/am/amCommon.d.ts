interface JavaString {
  new (value: StringLike): JavaString;

  includes(value: StringLike): boolean;

  split(separator: StringLike): JavaArray<JavaString>;

  getAsUtf8(): JavaString;

  getBytes(): JavaByteArray;
}

interface JavaByteArray {
  new (value: StringLike): JavaByteArray;
}

type StringLike = string | JavaString;

interface JavaArray<T = JavaString> {
  [index: number]: T | null | undefined;

  length: number;

  get(index: number): T | null | undefined;

  includes(value: T): boolean;

  asList(): any[];

  toArray(): T[];
}

interface JavaMap<Key = JavaString, Value = JavaString> {
  get(key: Key): Value | null;
}

interface Crypto {
  randomUUID(): string;
}

interface Utils {
  crypto: Crypto;
}

declare const utils: Utils;

declare const scriptName: string;

// Additional globals we discovered during ESLint work
declare const nodeState: NodeState;
declare const callbacks: Callbacks;
declare const callbacksBuilder: CallbacksBuilder;
declare const action: Action;
declare const requestParameters: RequestParameters;
declare const requestHeaders: RequestHeaders;
declare const idRepository: IdRepository;
declare let outcome: StringLike | undefined;

// OpenIDM with proper types
type Patch =
  | {
      operation: "add" | "replace";
      field: string;
      value: string | string[] | object;
    }
  | {
      operation: "remove";
      field: string;
    };

type IdmManagedObject =
  | "alpha_user"
  | "alpha_organization"
  | "providerProvisioningQueue"
  | "bravo_user"
  | "bravo_organization";

type IdmObjectPath = `managed/${IdmManagedObject}`;

type QueryResponse = {
  result: [
    {
      _id: string;
      _rev: string;
    } & object,
  ];
  resultCount: number;
  pagedResultsCookie: string | null;
  totalPagedResultsPolicy: string;
  totalPagedResults: number;
  remainingPagedResults: number;
};

declare const openidm: {
  read: (
    path: `${IdmObjectPath}/${string}`,
    params?: Record<string, string> | null,
    fields?: string[]
  ) => object | null;
  query: (
    path: IdmObjectPath,
    params: { _queryFilter: string },
    fields?: string[]
  ) => QueryResponse;
  create: (
    path: IdmObjectPath,
    newResourceId: string | null,
    content: Record<string, any> | null,
    params?: Record<string, string> | null,
    fields?: string[]
  ) => object;
  patch: (
    path: `${IdmObjectPath}/${string}`,
    revision: string | null,
    patch: Patch[],
    params?: Record<string, string> | null,
    fields?: string[]
  ) => object;
  delete: (
    path: `${IdmObjectPath}/${string}`,
    revision: string | null,
    params?: Record<string, string> | null,
    fields?: string[]
  ) => object;
};

declare function _nodeStateGet(
  key: StringLike
): Record<string, any> | JavaString | boolean | null | undefined;
declare function _nodeStateGet(
  key: "objectAttributes"
): Record<string, any> | null | undefined;

interface NodeState {
  get: typeof _nodeStateGet;
  getObject: (key: StringLike) => Object | null | undefined;
  putShared: (key: StringLike, value: any) => void;
  putTransient: (key: StringLike, value: any) => void;
}

interface RequestParameters {
  get: (key: StringLike) => JavaArray<JavaString> | null;
}

interface RequestHeaders {
  get: (key: StringLike) => JavaArray<JavaString> | null;
  containsKey: (key: StringLike) => boolean;
}

interface RequestCookies {
  get: (key: StringLike) => JavaArray<JavaString> | null;
  containsKey: (key: StringLike) => boolean;
}

interface SystemEnv {
  getProperty: (key: StringLike) => JavaString | null;
}
declare const systemEnv: SystemEnv;

interface Array<T> {
  concat<U extends T[]>(...items: U[]): T[];
  concat(...items: (T | JavaArray<T>)[]): T[];
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

interface HttpHeaders {
  "Content-Type"?: "application/json" | "application/x-www-form-urlencoded";
  "X-Api-Key"?: string;
  Authorization?: string;
  x_creation_datetime?: string;
  "x-correlation-id"?: string;
  "x-requesting-system-id"?: string;
}
interface HttpOptions {
  method: "GET" | "POST" | "PUT" | "DELETE";
  clientName?: string;
  headers?: HttpHeaders;
  token?: string;
  body?: object;
  form?: object;
}

interface HttpResponse {
  status: number;
  ok: boolean;
  statusText: string;
  headers: HttpHeaders;
  json(): object;
  text(): string;
}

interface HttpClient {
  send(
    requestUrl: string,
    httpOptions: HttpOptions
  ): {
    get(): HttpResponse;
  };
}
declare const httpClient: HttpClient;

type StringAttributePolicy =
  | {
      policyId: "minimum-length";
      params: { minLength: number };
      policyRequirements: ["MIN_LENGTH"];
    }
  | {
      policyId: "valid-date";
      params: {};
      policyRequirements: ["VALID_DATE"];
    };
type StringAttributePolicyRequirements =
  StringAttributePolicy["policyRequirements"][0];

type StringAttributePolicies = {
  policies: Array<StringAttributePolicy>;
  policyRequirements: Array<StringAttributePolicyRequirements>;
};

interface CallbacksBuilder {
  textInputCallback(message: StringLike): void;
  confirmationCallback(
    messageType: number,
    options: string[],
    defaultOption: number
  ): void;
  confirmationCallback(
    prompt: StringLike,
    messageType: number,
    options: string[],
    defaultOption: number
  ): void;
  /**
   * @param messageType - Type of message:
   * - 0: Information
   * - 1: Warning
   * - 2: Error
   * - 3: Unknown
   * - 4: Script
   */
  textOutputCallback(messageType: 0 | 1 | 2 | 4, message: StringLike): void;
  hiddenValueCallback(id: StringLike, value: StringLike): void;
  scriptTextOutputCallback(js: string): void;
  stringAttributeInputCallback(
    id: string,
    prompt: string,
    value: string | null,
    required: boolean
  ): void;
  stringAttributeInputCallback(
    id: string,
    prompt: string,
    value: string | null,
    required: boolean,
    policy: StringAttributePolicies | undefined,
    evaluatePolicy: boolean,
    failedPolicies: string[]
  ): void;
}

interface Callbacks {
  getTextInputCallbacks: () => JavaArray<string>;
  getHiddenValueCallbacks: () => JavaArray<string>;
  getStringAttributeInputCallbacks: () => JavaArray<string>;
  getConfirmationCallbacks: () => JavaArray<number>;
  isEmpty(): boolean;
}

interface Identity {
  getAttributeValues: (attributeName: string) => JavaArray<string>;
  setAttribute: (attributeName: string, value: [string] | []) => void;
  store: () => void;
}

interface IdRepository {
  getIdentity: (id: string) => Identity;
}

interface Action {
  goTo: (outcome: StringLike) => void;
  withHeader: (header: StringLike) => Action;
  withDescription: (html: StringLike) => Action;
  withStage: (stage: StringLike) => Action;
  putSessionProperty: (sessionKey: StringLike, value: any) => Action;
}
