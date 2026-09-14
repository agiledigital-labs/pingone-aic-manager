import { HARNESS_CALLBACK_ID } from "../../src/aic/constants.ts";
import type { HttpRequest, HttpResponse } from "../../src/aic/http.ts";
import type { AicIo } from "../../src/aic/tenant.ts";

const PLACEHOLDER_BASE = "https://tenant.example.com";

export interface FakeChain {
  io: AicIo;
  authPosts: string[];
  treesCreated: string[];
  subjectSource: string;
  subjectOutcomes: string[];
}

export function callbackResponse(type: string, authId: string): object {
  return {
    authId,
    callbacks: [
      {
        type,
        output: [],
        input: [{ name: "IDToken1", value: "" }],
      },
    ],
  };
}

export function finalResponse(
  outcome = "done",
  before: Record<string, unknown> = {},
  final: Record<string, unknown> = before
): object {
  return {
    callbacks: [
      {
        type: "HiddenValueCallback",
        output: [
          { name: "id", value: HARNESS_CALLBACK_ID },
          {
            name: "value",
            value: JSON.stringify({ outcome, before, final }),
          },
        ],
      },
    ],
  };
}

export function mockChain(
  options: {
    finishImmediately?: boolean;
    before?: Record<string, unknown>;
    responses?: readonly object[];
  } = {}
): FakeChain {
  const fake: FakeChain = {
    io: undefined as unknown as AicIo,
    authPosts: [],
    treesCreated: [],
    subjectSource: "",
    subjectOutcomes: [],
  };
  const seen = options.before ?? {};
  const finished = finalResponse("done", seen);
  const asking = {
    authId: "jwt-1",
    callbacks: [
      {
        type: "NameCallback",
        output: [{ name: "prompt", value: "User Name" }],
        input: [{ name: "IDToken1", value: "" }],
      },
    ],
  };

  fake.io = {
    aic(args) {
      if (args.includes("ctx") && args.includes("list")) {
        return Promise.resolve({
          status: 0,
          stdout: JSON.stringify([
            {
              current: true,
              name: "sandbox",
              theme: "sandbox",
              base_url: PLACEHOLDER_BASE,
            },
          ]),
          stderr: "",
        });
      }
      return Promise.resolve({ status: 0, stdout: "test-token\n", stderr: "" });
    },
    http(req: HttpRequest): Promise<HttpResponse> {
      const url = new URL(req.url);
      if (req.method === "GET") {
        return Promise.resolve(json(404, { code: 404 }));
      }
      if (req.method === "DELETE") {
        return Promise.resolve(json(200, {}));
      }
      if (req.method === "PUT") {
        const body = JSON.parse(String(req.body)) as Record<string, unknown>;
        if (url.pathname.includes("/trees/")) {
          fake.treesCreated.push(
            decodeURIComponent(url.pathname.split("/trees/")[1] ?? "")
          );
        }
        if (
          typeof body.script === "string" &&
          String(body.name).endsWith("-subject")
        ) {
          fake.subjectSource = Buffer.from(body.script, "base64").toString(
            "utf8"
          );
        }
        if (
          Array.isArray(body.outcomes) &&
          url.pathname.includes("/ScriptedDecisionNode/")
        ) {
          if (fake.subjectOutcomes.length === 0 && body.outcomes.length > 1) {
            fake.subjectOutcomes = body.outcomes as string[];
          }
        }
        return Promise.resolve(json(201, { _id: "created" }));
      }
      fake.authPosts.push(String(req.body));
      const response = options.responses?.[fake.authPosts.length - 1];
      if (response !== undefined) {
        return Promise.resolve(json(200, response));
      }
      if (options.finishImmediately === true || fake.authPosts.length > 1) {
        return Promise.resolve(json(200, finished));
      }
      return Promise.resolve(json(200, asking));
    },
  };
  return fake;
}

function json(status: number, body: unknown): HttpResponse {
  return { status, headers: [], body: JSON.stringify(body) };
}
