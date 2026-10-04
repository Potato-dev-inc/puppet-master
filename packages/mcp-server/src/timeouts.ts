/** Per-route call timeouts, WAIT_TIMEOUT recovery, and the progress-streaming operation wait. */
import { randomUUID } from 'node:crypto';
import { call, callWithTimeout } from './bridge-client.js';
import { ToolCallError } from './errors.js';
import type { BridgeClient } from './types.js';

const DEFAULT_AGENT_WAIT_MS = 120_000;

export function waitTimeoutMs(args: Record<string, unknown>): number {
  const timeout = typeof args.timeout_ms === 'number' ? args.timeout_ms : 120_000;
  return Math.min(timeout + 15_000, 310_000);
}

export function ensureIdempotencyKey(args: Record<string, unknown>): Record<string, unknown> {
  if (typeof args.idempotency_key === 'string' && args.idempotency_key.trim().length > 0) {
    return args;
  }
  return { ...args, idempotency_key: randomUUID() };
}

export function toolTimeoutMs(path: string, args: Record<string, unknown>): number {
  if (path === '/agents/run') {
    if (args.background === true) return 30_000;
    return Math.min((typeof args.wait_ms === 'number' ? args.wait_ms : DEFAULT_AGENT_WAIT_MS) + 15_000, 320_000);
  }
  if (path === '/agents/followup') {
    return Math.min((typeof args.wait_ms === 'number' ? args.wait_ms : 120_000) + 15_000, 320_000);
  }
  if (
    path === '/agents/wait'
    || path.includes('/panes/wait')
    || (path.includes('/operations/') && path.endsWith('/wait'))
  ) {
    return waitTimeoutMs(args);
  }
  return 30_000;
}

export async function waitTimeoutError(
  client: BridgeClient,
  path: string,
  args: Record<string, unknown>,
  err: unknown,
): Promise<unknown> {
  const timedOut = err instanceof Error && (err.name === 'AbortError' || /timed out|10060/i.test(err.message));
  if (!timedOut) return err;
  const key = typeof args.idempotency_key === 'string' ? args.idempotency_key : undefined;
  if (key) {
    try {
      const query = new URLSearchParams({ idempotency_key: key });
      if (typeof args.project_path === 'string') query.set('project_path', args.project_path);
      const snap = await call<Record<string, unknown>>(client, 'GET', `/operations/by-key?${query.toString()}`);
      const status = String(snap.status ?? '');
      const handle = String(snap.agent_run_id ?? snap.operation_id ?? args.handle ?? '');
      const outcome =
        status === 'completed' || status === 'failed' || status === 'cancelled'
          ? 'completed_delivery_failed'
          : ['queued', 'starting', 'running', 'waiting_input'].includes(status)
            ? 'pending'
            : 'not_dispatched';
      return new ToolCallError({
        code: 'WAIT_TIMEOUT',
        message: 'transport timed out; use next_action instead of retrying with a new idempotency key',
        recoverable: true,
        context: {
          outcome,
          handle,
          operation_id: snap.operation_id,
          turn_id: `turn-${String(snap.turn_index ?? 0)}`,
          idempotency_key: key,
          path,
          result: outcome === 'completed_delivery_failed' ? snap.result : undefined,
          next_action: {
            tool: 'wait_agents',
            handles: handle ? [handle] : [],
            until: ['completed', 'failed', 'cancelled', 'waiting_input'],
          },
        },
      });
    } catch {
      /* fall through to handle-based recovery */
    }
  }
  const handle = [args.handle, args.worker_id, args.name].find(
    (value): value is string => typeof value === 'string' && value.length > 0,
  );
  if (handle && (path === '/agents/followup' || path === '/agents/run')) {
    return new ToolCallError({
      code: 'WAIT_TIMEOUT',
      message: `transport timed out; call wait_agents with handle ${handle} instead of retrying the same task`,
      recoverable: true,
      context: {
        outcome: 'unknown',
        handle,
        path,
        suggestion: 'wait_agents',
      },
    });
  }
  return err;
}

export async function waitForOperationWithProgress(client: BridgeClient, path: string, body: Record<string, unknown>, timeoutMs: number, signal: AbortSignal, onProgress: (snapshot: unknown) => Promise<void>): Promise<unknown> {
  const deadline = Date.now() + timeoutMs;
  let revision = typeof body.after_revision === 'number' ? body.after_revision : -1;
  const until = Array.isArray(body.until) ? body.until.filter((item): item is string => typeof item === 'string') : undefined;
  const waitBody = Object.fromEntries(Object.entries(body).filter(([key]) => key !== 'until'));
  let lastResponse: { snapshot?: Record<string, unknown>; reason?: string } | undefined;
  while (Date.now() < deadline) {
    if (signal.aborted) throw new Error('request cancelled');
    const remainingMs = deadline - Date.now();
    const response = await callWithTimeout<{ snapshot?: Record<string, unknown>; reason?: string }>(client, 'POST', path, { ...waitBody, after_revision: revision, timeout_ms: Math.min(4_000, remainingMs) }, Math.min(20_000, remainingMs + 1_000), signal);
    lastResponse = response;
    const snapshot: Record<string, unknown> = response.snapshot ?? response as Record<string, unknown>;
    const nextRevision = typeof snapshot.revision === 'number' ? snapshot.revision : revision;
    const status = String(snapshot.status ?? '');
    if (nextRevision !== revision || ['completed', 'failed', 'cancelled'].includes(status)) await onProgress(snapshot);
    revision = nextRevision;
    if (['completed', 'failed', 'cancelled'].includes(status) || response.reason === 'matched_state') return response;
    if (until && until.includes(status)) return { ...response, reason: 'matched_state' };
  }
  if (lastResponse) return { snapshot: lastResponse.snapshot ?? lastResponse, reason: 'timeout' };
  const first = await callWithTimeout<{ snapshot?: Record<string, unknown> }>(client, 'POST', path, { ...waitBody, after_revision: revision, timeout_ms: 1 }, 5_000, signal);
  return { snapshot: first.snapshot ?? first, reason: 'timeout' };
}
