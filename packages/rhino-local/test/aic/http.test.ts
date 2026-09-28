import http from "node:http";
import { afterEach, describe, expect, it } from "vitest";
import { sendHttp } from "../../src/aic/http.ts";

describe("sendHttp", () => {
  let server: http.Server | undefined;
  let seen: { rawHeaders: string[]; url: string; method: string; body: string } | undefined;

  afterEach(async () => {
    await close(server);
    server = undefined;
  });

  it("sends duplicate header names as separate lines, not a comma join", async () => {
    server = await listen((req, body) => {
      seen = { rawHeaders: req.rawHeaders, url: req.url ?? "", method: req.method ?? "", body };
    });
    const address = server.address();
    if (address === null || typeof address === "string") {
      throw new Error("expected TCP address");
    }
    await sendHttp({
      url: `http://127.0.0.1:${address.port}/authenticate?authIndexType=service`,
      method: "POST",
      headerLines: [
        ["Accept-API-Version", "protocol=1.0,resource=2.1"],
        ["X-Aic-Probe", "alpha"],
        ["X-Aic-Probe", "bravo"],
      ],
      body: "{}",
    });
    expect(seen?.method).toBe("POST");
    const probe = valuesOf(seen?.rawHeaders ?? [], "x-aic-probe");
    expect(probe).toEqual(["alpha", "bravo"]);
  });
});

function valuesOf(rawHeaders: string[], name: string): string[] {
  const values: string[] = [];
  for (let index = 0; index < rawHeaders.length; index += 2) {
    if (rawHeaders[index]?.toLowerCase() === name) {
      const value = rawHeaders[index + 1];
      if (value !== undefined) {
        values.push(value);
      }
    }
  }
  return values;
}

function listen(
  onRequest: (req: http.IncomingMessage, body: string) => void
): Promise<http.Server> {
  const server = http.createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on("data", (chunk: Buffer | string) => {
      chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
    });
    req.on("end", () => {
      onRequest(req, Buffer.concat(chunks).toString("utf8"));
      res.statusCode = 200;
      res.setHeader("Content-Type", "application/json");
      res.end("{}");
    });
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

function close(server: http.Server | undefined): Promise<void> {
  if (server === undefined) {
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}
