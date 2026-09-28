import { request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";
import type { IncomingMessage, RequestOptions } from "node:http";

export interface HttpResponse {
  status: number;
  headers: Array<[string, string]>;
  body: string;
}

export interface HttpRequest {
  url: string;
  method: string;
  /** Repeated names are sent as repeated header lines, not comma-joined. */
  headerLines: Array<[string, string]>;
  body?: string;
  timeoutMs?: number;
}

/**
 * HTTP/1.1 request that preserves duplicate header names. Node's
 * `setHeader(name, [a,b])` comma-joins; AM `requestHeaders` stores one
 * element per occurrence (`docs/api/12-script-bindings-matrix.md`), so the
 * authenticate call has to write each line itself.
 */
export function sendHttp(req: HttpRequest): Promise<HttpResponse> {
  const url = new URL(req.url);
  const isHttps = url.protocol === "https:";
  const requestFn = isHttps ? httpsRequest : httpRequest;
  const body = req.body ?? "";
  const headerLines = withHostAndLength(url, req.headerLines, Buffer.byteLength(body));

  const options: RequestOptions = {
    protocol: url.protocol,
    hostname: url.hostname,
    port: url.port === "" ? undefined : url.port,
    path: `${url.pathname}${url.search}`,
    method: req.method,
    timeout: req.timeoutMs ?? 30_000,
    // Leave headers empty; we write them on the wire below.
    headers: {},
  };

  return new Promise((resolve, reject) => {
    const request = requestFn(options, (response) => {
      collect(response).then(resolve, reject);
    });
    request.on("timeout", () => {
      request.destroy(new Error(`rhino-local: HTTP timeout after ${options.timeout}ms`));
    });
    request.on("error", reject);
    writeHeaderLines(request, headerLines);
    if (body.length > 0) {
      request.write(body);
    }
    request.end();
  });
}

function withHostAndLength(
  url: URL,
  headerLines: Array<[string, string]>,
  length: number
): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  let hasHost = false;
  let hasLength = false;
  let hasConnection = false;
  for (const [name, value] of headerLines) {
    const lower = name.toLowerCase();
    if (lower === "host") {
      hasHost = true;
    }
    if (lower === "content-length") {
      hasLength = true;
    }
    if (lower === "connection") {
      hasConnection = true;
    }
    out.push([name, value]);
  }
  if (!hasHost) {
    const host = url.port === "" ? url.hostname : `${url.hostname}:${url.port}`;
    out.unshift(["Host", host]);
  }
  if (!hasLength) {
    out.push(["Content-Length", String(length)]);
  }
  if (!hasConnection) {
    out.push(["Connection", "close"]);
  }
  return out;
}

function writeHeaderLines(
  request: { setHeader(name: string, value: unknown): void },
  headerLines: Array<[string, string]>
): void {
  const grouped = new Map<string, string[]>();
  const order: string[] = [];
  for (const [name, value] of headerLines) {
    const existing = grouped.get(name);
    if (existing === undefined) {
      grouped.set(name, [value]);
      order.push(name);
    } else {
      existing.push(value);
    }
  }
  for (const name of order) {
    const values = grouped.get(name);
    if (values === undefined) {
      continue;
    }
    const single = values[0];
    request.setHeader(name, values.length === 1 && single !== undefined ? single : values);
  }
}

function collect(response: IncomingMessage): Promise<HttpResponse> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    response.on("data", (chunk: Buffer | string) => {
      chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
    });
    response.on("error", reject);
    response.on("end", () => {
      const headers: Array<[string, string]> = [];
      const raw = response.rawHeaders;
      for (let index = 0; index < raw.length; index += 2) {
        const name = raw[index];
        const value = raw[index + 1];
        if (name === undefined || value === undefined) {
          continue;
        }
        headers.push([name, value]);
      }
      resolve({
        status: response.statusCode ?? 0,
        headers,
        body: Buffer.concat(chunks).toString("utf8"),
      });
    });
  });
}

export function headerValues(headers: Array<[string, string]>, name: string): string[] {
  const lower = name.toLowerCase();
  const values: string[] = [];
  for (const [key, value] of headers) {
    if (key.toLowerCase() === lower) {
      values.push(value);
    }
  }
  return values;
}
