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
    { _queryFilter: string },
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

declare const nodeState: NodeState;

interface Action {
  goTo: (outcome: StringLike) => void;
  withHeader: (header: StringLike) => Action;
  withDescription: (html: StringLike) => Action;
  withStage: (stage: StringLike) => Action;
  putSessionProperty: (sessionKey: StringLike, value: any) => Action;
}
declare const action: Action;

declare let outcome: StringLike | undefined;

declare const callbacksBuilder: CallbacksBuilder;

declare const callbacks: Callbacks;

declare const requestParameters: RequestParameters;
declare const requestHeaders: RequestHeaders;

declare const idRepository: IdRepository;

interface ExistingSession {
  Principal: string;
}

declare const existingSession: ExistingSession | undefined;