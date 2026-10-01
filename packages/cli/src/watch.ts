import { readBridgePort } from '@puppet-master/shared/bridge-port';

export type WatchTerminalStatus =
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'waiting_input'
  | 'needs_input'
  | 'timeout';

export interface WatchLine {
  operation_id: string;
  project_path?: string;
  status: string;
  revision?: number;
  result?: string | null;
  error?: unknown;
}

const TERMINAL = new Set(['completed', 'failed', 'cancelled']);
const NEEDS_INPUT = new Set(['waiting_input', 'needs_input']);

function exitCodeForStatus(status: string): number {
  if (status === 'completed') return 0;
  if (TERMINAL.has(status) && status !== 'completed') return 1;
  if (NEEDS_INPUT.has(status)) return 2;
  return 3;
}

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
      throw new Error(`bridge returned non-JSON (${response.status}): ${text.slice(0, 200)}`);
    }
    if (!response.ok) {
      const err = parsed as { message?: string; error?: string };
      throw new Error(err.message ?? err.error ?? `bridge error ${response.status}`);
    }
    return parsed;
  } finally {
    clearTimeout(timer);
  }
}

async function resolveOperation(
  idOrHandle: string,
  projectPath?: string,
): Promise<{ operationId: string; projectPath: string; snapshot: Record<string, unknown> }> {
  const query = new URLSearchParams();
  if (projectPath) query.set('project_path', projectPath);
  const projectQuery = query.toString() ? `?${query.toString()}` : '';

  try {
    const snap = await bridgeJson<Record<string, unknown>>(
      'GET',
      `/operations/${encodeURIComponent(idOrHandle)}${projectQuery}`,
    );
    const project = String(snap.project_path ?? projectPath ?? '');
    return {
      operationId: String(snap.operation_id ?? idOrHandle),
      projectPath: project,
      snapshot: snap,
    };
  } catch {
    const handleQuery = new URLSearchParams({ agent_run_id: idOrHandle });
    if (projectPath) handleQuery.set('project_path', projectPath);
    const snap = await bridgeJson<Record<string, unknown>>(
      'GET',
      `/operations/by-handle?${handleQuery.toString()}`,
    );
    return {
      operationId: String(snap.operation_id ?? idOrHandle),
      projectPath: String(snap.project_path ?? projectPath ?? ''),
      snapshot: snap,
    };
  }
}

function watchLine(snapshot: Record<string, unknown>, operationId: string, projectPath: string): WatchLine {
  const status = String(snapshot.status ?? 'unknown');
  return {
    operation_id: operationId,
    project_path: projectPath || undefined,
    status,
    revision: typeof snapshot.revision === 'number' ? snapshot.revision : undefined,
    result: (snapshot.result as string | null | undefined) ?? null,
    error: snapshot.error,
  };
}

function isDone(status: string): boolean {
  return TERMINAL.has(status) || NEEDS_INPUT.has(status);
}

export async function runWatch(options: {
  ids: string[];
  projectPath?: string;
  timeoutMs: number;
  stream: boolean;
}): Promise<number> {
  const targets = await Promise.all(
    options.ids.map(async (id) => {
      const resolved = await resolveOperation(id, options.projectPath);
      return {
        operationId: resolved.operationId,
        projectPath: resolved.projectPath,
        revision: typeof resolved.snapshot.revision === 'number' ? resolved.snapshot.revision : 0,
        lastStatus: String(resolved.snapshot.status ?? 'unknown'),
      };
    }),
  );

  const deadline = Date.now() + options.timeoutMs;
  const lastPrinted = new Map<string, string>();

  while (Date.now() < deadline) {
    const remaining = targets.filter((t) => !isDone(t.lastStatus));
    if (remaining.length === 0) break;

    const sliceMs = Math.min(60_000, Math.max(1_000, deadline - Date.now()));
    await Promise.all(
      remaining.map(async (target) => {
        const body: Record<string, unknown> = {
          after_revision: target.revision,
          until: ['completed', 'failed', 'cancelled', 'waiting_input', 'needs_input'],
          timeout_ms: sliceMs,
        };
        const query = target.projectPath
          ? `?project_path=${encodeURIComponent(target.projectPath)}`
          : '';
        try {
          const wait = await bridgeJson<{ snapshot: Record<string, unknown> }>(
            'POST',
            `/operations/${encodeURIComponent(target.operationId)}/wait${query}`,
            body,
            sliceMs + 25_000,
          );
          const snap = wait.snapshot ?? {};
          target.revision =
            typeof snap.revision === 'number' ? snap.revision : target.revision;
          target.lastStatus = String(snap.status ?? target.lastStatus);
          if (options.stream) {
            const key = `${target.operationId}:${target.revision}:${target.lastStatus}`;
            if (!lastPrinted.has(target.operationId) || lastPrinted.get(target.operationId) !== key) {
              lastPrinted.set(target.operationId, key);
              console.log(
                JSON.stringify(
                  watchLine(snap, target.operationId, target.projectPath),
                ),
              );
            }
          }
        } catch (err) {
          if (err instanceof Error && err.name === 'AbortError') {
            return;
          }
          throw err;
        }
      }),
    );
  }

  const finals = await Promise.all(
    targets.map(async (target) => {
      const query = target.projectPath
        ? `?project_path=${encodeURIComponent(target.projectPath)}`
        : '';
      const snap = await bridgeJson<Record<string, unknown>>(
        'GET',
        `/operations/${encodeURIComponent(target.operationId)}${query}`,
      );
      return watchLine(snap, target.operationId, target.projectPath);
    }),
  );

  if (!options.stream) {
    const last = finals[finals.length - 1];
    if (last) console.log(JSON.stringify(last));
  } else {
    for (const line of finals) {
      if (!isDone(line.status)) {
        console.log(JSON.stringify({ ...line, status: 'timeout' }));
      }
    }
  }

  if (finals.some((line) => !isDone(line.status))) {
    return 3;
  }
  if (finals.some((line) => NEEDS_INPUT.has(line.status))) {
    return 2;
  }
  if (finals.every((line) => line.status === 'completed')) {
    return 0;
  }
  return 1;
}
