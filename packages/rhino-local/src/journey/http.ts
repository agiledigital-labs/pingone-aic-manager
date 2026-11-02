/** HTTP-shaped adapter for aic/run.ts invokeJourney/driveJourney. No server or test framework. */
import type { HttpRequest, HttpResponse } from "../aic/http.ts";
import type { JourneyEmulator } from "./engine.ts";
import type { JsonValue } from "../protocol.ts";
export function journeyHttp(engine: JourneyEmulator): (request: HttpRequest) => Promise<HttpResponse> {
  return async (request) => {
    const url = new URL(request.url);
    const headers: Record<string, string[]> = Object.create(null) as Record<string, string[]>;
    for (const [key, value] of request.headerLines) (headers[key] ??= []).push(value);
    const cookie = request.headerLines.filter(([name]) => name.toLowerCase() === "cookie").map(([, value]) => value).join("; ");
    const cookies = Object.fromEntries(cookie.split(";").filter(Boolean).map((item) => { const at = item.indexOf("="); return [item.slice(0, at).trim(), item.slice(at + 1).trim()]; }));
    const result = await engine.request({ method: request.method as "POST", path: url.pathname,
      query: { ...Object.fromEntries([...new Set(url.searchParams.keys())].map((key) => [key, url.searchParams.getAll(key)])), authIndexType: url.searchParams.get("authIndexType") ?? "", authIndexValue: url.searchParams.get("authIndexValue") ?? "" },
      body: request.body ? JSON.parse(request.body) as JsonValue : {}, headers, cookies });
    return { status: result.status, headers: [["content-type", "application/json"]], body: result.rawBody ?? JSON.stringify(result.body) };
  };
}
