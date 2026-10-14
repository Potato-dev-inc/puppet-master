/** JSON replies shaped like relay/server's: errors are `{ "error": "..." }`. */

export function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

/** A body that is already JSON text. */
export function jsonText(text: string, status = 200): Response {
  return new Response(text, { status, headers: { "content-type": "application/json" } });
}

/**
 * An empty success. The Workers runtime gzips responses for clients that accept it, and does so
 * even for a bodiless `204`, which ureq (the HTTP client the app uses) then fails to read to its
 * end ("unexpected end of file"). `content-encoding: identity` says "leave it alone".
 */
export function empty(status: 201 | 202 | 204): Response {
  return new Response(null, { status, headers: { "content-encoding": "identity" } });
}

export function refuse(status: number, error: string): Response {
  return json({ error }, status);
}

/** Thrown inside the Durable Object to stop a request with a refusal. */
export class Refusal extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
  response(): Response {
    return refuse(this.status, this.message);
  }
}

/**
 * The request body, or a 413 when it is over `max` bytes. The `Content-Length` is checked first and
 * the stream is counted while it is read, so a lying header does not help.
 */
export async function readBody(request: Request, max: number): Promise<Uint8Array<ArrayBuffer> | Response> {
  const declared = Number(request.headers.get("content-length") ?? "0");
  if (declared > max) return refuse(413, "request body too large");
  if (!request.body) return new Uint8Array(0);
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > max) {
      await reader.cancel();
      return refuse(413, "request body too large");
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let at = 0;
  for (const chunk of chunks) {
    out.set(chunk, at);
    at += chunk.byteLength;
  }
  return out;
}

/** `after=` / `wait=` style unsigned integers; anything else is ignored, as in relay/server. */
export function queryU64(url: URL, key: string): number | undefined {
  const raw = url.searchParams.get(key);
  if (raw === null || !/^\d+$/.test(raw)) return undefined;
  const n = Number(raw);
  return Number.isSafeInteger(n) ? n : undefined;
}
