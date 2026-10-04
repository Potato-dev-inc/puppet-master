import { readBridgePort } from '@puppet-master/shared/bridge-port';

export type WatchTerminalStatus =
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'waiting_input'
  | 'needs_input'
  | 'timeout'
  | 'error';

export interface WatchLine {
  operation_id: string;
  project_path?: string;
  status: string;
  revision?: number;
  result?: string | null;
  error?: unknown;
  /** `timeout` lines: the last status the watcher saw before the deadline. */
  last_status?: string;
  /** `timeout` lines: true when the bridge could not be reached at the end. */
  bridge_unreachable?: boolean;
}

/**
 * Exit codes: 0 completed, 1 failed/cancelled, 2 needs input (waiting_input), 3 timeout,
 * 4 error (unknown operation, rejected request, bad usage).
 */
export const EXIT_COMPLETED = 0;
export const EXIT_FAILED = 1;
export const EXIT_NEEDS_INPUT = 2;
export const EXIT_TIMEOUT = 3;
export const EXIT_ERROR = 4;

const TERMINAL = new Set(['completed', 'failed', 'cancelled']);
const NEEDS_INPUT = new Set(['waiting_input', 'needs_input']);

function shellQuote(value: string): string {
  if (
    value.length > 0 &&
    /^[\w./:\\-]+$/.test(value) &&
    !value.includes(' ')
  ) {
    return value;
  }
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

export function formatWatchCommand(operationId: string, projectPath?: string): string {
  let cmd = `npx puppet-master watch ${shellQuote(operationId)}`;
  if (projectPath?.trim()) {
    cmd += ` --project-path ${shellQuote(projectPath.trim())}`;
  }
  return cmd;
}

/** The bridge answered with an HTTP error. 5xx is transient (retried); 4xx is a real answer. */
class BridgeHttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string | undefined,
    message: string,
  ) {
    super(message);
  }
}

async function bridgeJson<T>(
  method: string,
  path: string,
  body?: Record<string, unknown>,
  timeoutMs = 30_000,
): Promise<T> {
  const { host, port } = await readBridgePort();
  const url = new URL(path, `http://${host}:${port}`);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const init: RequestInit = {
      method,
      signal: controller.signal,
      headers: { 'Content-Type': 'application/json' },
    };
    if (body !== undefined) {
      init.body = JSON.stringify(body);
    }
    const response = await fetch(url, init);
    const text = await response.text();
    let parsed: T;
    try {
      parsed = JSON.parse(text) as T;
    } catch {
      throw new BridgeHttpError(
        response.ok ? 502 : response.status,
        undefined,
        `bridge returned non-JSON (${response.status}): ${text.slice(0, 200)}`,
      );
    }
    if (!response.ok) {
      const err = parsed as { message?: string; error?: string; code?: string };
      throw new BridgeHttpError(
        response.status,
        err.code,
        err.message ?? err.error ?? `bridge error ${response.status}`,
      );
    }
    return parsed;
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Anything that is not a definite 4xx answer: refused/reset connections, a missing port file
 * (bridge not started yet), aborted long-polls, 5xx. The operation is durable, so wait again.
 */
function isRetryable(err: unknown): boolean {
  return !(err instanceof BridgeHttpError) || err.status >= 500;
}

function snapshotFields(snapshot: Record<string, unknown>) {
  return {
    status: String(snapshot.status ?? ''),
    revision: typeof snapshot.revision === 'number' ? snapshot.revision : undefined,
  };
}

function watchLine(snapshot: Record<string, unknown>, operationId: string, projectPath: string): WatchLine {
  const { status, revision } = snapshotFields(snapshot);
  return {
    operation_id: operationId,
    project_path: projectPath || undefined,
    status: status || 'unknown',
    revision,
    result: (snapshot.result as string | null | undefined) ?? null,
    error: snapshot.error,
  };
}

function isDone(status: string): boolean {
  return TERMINAL.has(status) || NEEDS_INPUT.has(status);
}

interface Target {
  operationId: string;
  projectPath: string;
  revision: number;
  snapshot: Record<string, unknown>;
  unreachable: boolean;
  fatal?: { code: string; message: string };
}

export interface WatchOptions {
  ids: string[];
  projectPath?: string;
  /** Overall deadline. At the deadline the watcher reports `timeout` and exits 3. */
  timeoutMs: number;
  stream: boolean;
  /** Result sink; defaults to one JSON line on stdout. */
  out?: (line: string) => void;
  /** Reconnect backoff bounds (ms); tests shrink them. */
  retryBaseMs?: number;
  retryMaxMs?: number;
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, Math.max(0, ms)));

async function resolveOperation(
  idOrHandle: string,
  projectPath?: string,
): Promise<{ operationId: string; projectPath: string; snapshot: Record<string, unknown> }> {
  const query = new URLSearchParams();
  if (projectPath) query.set('project_path', projectPath);
  const projectQuery = query.toString() ? `?${query.toString()}` : '';
  let snap: Record<string, unknown>;
  try {
    snap = await bridgeJson<Record<string, unknown>>(
      'GET',
      `/operations/${encodeURIComponent(idOrHandle)}${projectQuery}`,
    );
  } catch (err) {
    // Only "no such operation" means the argument may be an agent_run handle.
    if (!(err instanceof BridgeHttpError) || err.status !== 404) throw err;
    const handleQuery = new URLSearchParams({ agent_run_id: idOrHandle });
    if (projectPath) handleQuery.set('project_path', projectPath);
    snap = await bridgeJson<Record<string, unknown>>(
      'GET',
      `/operations/by-handle?${handleQuery.toString()}`,
    );
  }
  return {
    operationId: String(snap.operation_id ?? idOrHandle),
    projectPath: String(snap.project_path ?? projectPath ?? ''),
    snapshot: snap,
  };
}

/**
 * Block until every operation is terminal (or needs input), then print one JSON result per
 * operation. The bridge is the source of truth and operations are durable, so a bridge that
 * restarts or is briefly unreachable is waited out: the watcher re-reads the port file,
 * resumes the long-poll from the last revision it saw, and gives up only at the deadline.
 * The watcher needs no MCP connection or ownership; it is a read-only observer, and stopping
 * it never cancels the operation.
 */
export async function runWatch(options: WatchOptions): Promise<number> {
  const out = options.out ?? ((line: string) => console.log(line));
  const retryBase = options.retryBaseMs ?? 500;
  const retryMax = options.retryMaxMs ?? 5_000;
  const deadline = Date.now() + options.timeoutMs;
  const remaining = () => deadline - Date.now();
  const printed = new Map<string, string>();

  const emitStream = (target: Target) => {
    if (!options.stream) return;
    const { status, revision } = snapshotFields(target.snapshot);
    const key = `${revision}:${status}`;
    if (printed.get(target.operationId) === key) return;
    printed.set(target.operationId, key);
    out(JSON.stringify(watchLine(target.snapshot, target.operationId, target.projectPath)));
  };

  const track = async (id: string): Promise<Target> => {
    const target: Target = {
      operationId: id,
      projectPath: options.projectPath ?? '',
      revision: 0,
      snapshot: {},
      unreachable: false,
    };
    let backoff = retryBase;
    /** Returns true when the failure is final for this target. */
    const failed = async (err: unknown): Promise<boolean> => {
      if (!isRetryable(err)) {
        const http = err as BridgeHttpError;
        target.fatal = { code: http.code ?? `HTTP_${http.status}`, message: http.message };
        return true;
      }
      target.unreachable = true;
      await sleep(Math.min(backoff, Math.max(0, remaining())));
      backoff = Math.min(backoff * 2, retryMax);
      return false;
    };

    // Resolve the id (or handle) to an operation, waiting out a bridge that is not up yet.
    while (remaining() > 0 && !target.snapshot.status) {
      try {
        const resolved = await resolveOperation(id, options.projectPath);
        target.operationId = resolved.operationId;
        target.projectPath = resolved.projectPath;
        target.snapshot = resolved.snapshot;
        target.revision = snapshotFields(resolved.snapshot).revision ?? 0;
        target.unreachable = false;
        backoff = retryBase;
      } catch (err) {
        if (await failed(err)) return target;
      }
    }
    if (!target.snapshot.status) return target;
    emitStream(target);

    const query = target.projectPath
      ? `?project_path=${encodeURIComponent(target.projectPath)}`
      : '';
    while (!isDone(snapshotFields(target.snapshot).status) && remaining() > 0) {
      const sliceMs = Math.min(60_000, Math.max(1, remaining()));
      try {
        const wait = await bridgeJson<{ snapshot: Record<string, unknown> }>(
          'POST',
          `/operations/${encodeURIComponent(target.operationId)}/wait${query}`,
          {
            after_revision: target.revision,
            until: ['completed', 'failed', 'cancelled', 'waiting_input', 'needs_input'],
            timeout_ms: sliceMs,
          },
          Math.min(sliceMs + 25_000, remaining() + 2_000),
        );
        target.unreachable = false;
        backoff = retryBase;
        const snap = wait.snapshot ?? {};
        const fields = snapshotFields(snap);
        if (fields.revision !== undefined && fields.revision < target.revision) {
          continue; // stale reply; keep the newer view
        }
        target.snapshot = snap;
        target.revision = fields.revision ?? target.revision;
        emitStream(target);
      } catch (err) {
        if (await failed(err)) return target;
      }
    }
    return target;
  };

  const targets = await Promise.all(options.ids.map(track));

  let timedOut = false;
  let errored = false;
  let needsInput = false;
  let failedAny = false;
  for (const target of targets) {
    const status = snapshotFields(target.snapshot).status;
    if (target.fatal) {
      errored = true;
      out(
        JSON.stringify({
          operation_id: target.operationId,
          project_path: target.projectPath || undefined,
          status: 'error',
          error: target.fatal,
        } satisfies WatchLine),
      );
    } else if (!status || !isDone(status)) {
      timedOut = true;
      out(
        JSON.stringify({
          operation_id: target.operationId,
          project_path: target.projectPath || undefined,
          status: 'timeout',
          revision: target.revision || undefined,
          last_status: status || undefined,
          bridge_unreachable: target.unreachable || undefined,
        } satisfies WatchLine),
      );
    } else {
      if (!options.stream) {
        out(JSON.stringify(watchLine(target.snapshot, target.operationId, target.projectPath)));
      }
      if (NEEDS_INPUT.has(status)) needsInput = true;
      else if (status !== 'completed') failedAny = true;
    }
  }

  if (errored) return EXIT_ERROR;
  if (timedOut) return EXIT_TIMEOUT;
  if (needsInput) return EXIT_NEEDS_INPUT;
  return failedAny ? EXIT_FAILED : EXIT_COMPLETED;
}
