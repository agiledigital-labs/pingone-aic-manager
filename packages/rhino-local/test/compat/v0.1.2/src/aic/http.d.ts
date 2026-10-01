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
export declare function sendHttp(req: HttpRequest): Promise<HttpResponse>;
export declare function headerValues(headers: Array<[string, string]>, name: string): string[];
