/** HTTP client for the local bridge: base URL discovery, plain and timed calls, GET refresh. */
import { readBridgePort, readLocalSecretHeaders } from '@puppet-master/shared/bridge-port';
import { bridgeHttpToolError } from './errors.js';
import type { BridgeClient, ClientRef } from './types.js';
import { log } from './util.js';

export async function makeClient(sessionId: string): Promise<BridgeClient> {
  const { host, port } = await readBridgePort();
  return { baseUrl: `http://${host}:${port}`, sessionId };
}

export async function call<T>(client: BridgeClient, method: string, path: string, body?: unknown): Promise<T> {
  const url = `${client.baseUrl}${path}`;
  let res: Response;
  try {
    res = await fetch(url, {
      method,
      headers: { ...(await readLocalSecretHeaders()), 'X-Puppet-Master-Session': client.sessionId, ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
  } catch (err) {
    throw new Error(`bridge ${method} ${path} -> fetch failed at ${client.baseUrl}: ${err instanceof Error ? err.message : String(err)}`);
  }
  if (!res.ok) {
    const text = await res.text();
    throw bridgeHttpToolError(res.status, text, `${method} ${path}`);
  }
  return (await res.json()) as T;
}

export async function callWithRefresh<T>(
  clientRef: ClientRef,
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  try {
    return await call<T>(clientRef.current, method, path, body);
  } catch (err) {
    if (method !== 'GET') throw err;
    const message = err instanceof Error ? err.message : String(err);
    if (!message.includes('fetch failed')) throw err;
    const next = await makeClient(clientRef.current.sessionId);
    clientRef.current = next;
    log('refreshed bridge client', next.baseUrl);
    return await call<T>(clientRef.current, method, path, body);
  }
}

export async function callWithTimeout<T>(
  client: BridgeClient,
  method: string,
  path: string,
  body: unknown,
  timeoutMs: number,
  externalSignal?: AbortSignal,
): Promise<T> {
  const url = `${client.baseUrl}${path}`;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const abort = () => controller.abort();
  externalSignal?.addEventListener('abort', abort, { once: true });
  try {
    const res = await fetch(url, {
      method,
      headers: { ...(await readLocalSecretHeaders()), 'X-Puppet-Master-Session': client.sessionId, ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    });
    if (!res.ok) {
      const text = await res.text();
      throw bridgeHttpToolError(res.status, text, `${method} ${path}`);
    }
    return (await res.json()) as T;
  } catch (err) {
    if (err instanceof Error && err.name === 'AbortError') {
      throw new Error(`bridge ${method} ${path} timed out after ${timeoutMs}ms`);
    }
    throw err;
  } finally {
    clearTimeout(timer);
    externalSignal?.removeEventListener('abort', abort);
  }
}
