import { spawn } from 'node:child_process';
import { existsSync, statSync } from 'node:fs';
import { createConnection } from 'node:net';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  BRIDGE_HTTP_PORT_RANGE,
  findReusableWorkerPane,
  getDefaultTerminalAgentType,
  type PaneInfo,
} from '@puppet-master/shared';
import { readAllBridgePorts } from '@puppet-master/shared/bridge-port';

const __dirname = dirname(fileURLToPath(import.meta.url));
const VITE_DEV_PORT = 1420;

export interface WorkerLaunchOptions {
  agentType?: string;
  cwd?: string;
  paneId?: string;
  cols?: number;
  rows?: number;
  forceNew?: boolean;
}

function repoRoot(): string {
  return resolve(__dirname, '..', '..', '..');
}

function appBinary(root: string, profile: 'debug' | 'release'): string | null {
  const name = process.platform === 'win32' ? 'puppet_master_app.exe' : 'puppet_master_app';
  const binary = join(root, 'packages', 'app', 'src-tauri', 'target', profile, name);
  return existsSync(binary) ? binary : null;
}

function portInUse(port: number): Promise<boolean> {
  return new Promise((resolvePort) => {
    const socket = createConnection({ port, host: '127.0.0.1' }, () => {
      socket.end();
      resolvePort(true);
    });
    socket.setTimeout(400, () => {
      socket.destroy();
      resolvePort(false);
    });
    socket.on('error', () => resolvePort(false));
  });
}

function releaseBinary(root: string): string | null {
  const binary = appBinary(root, 'release');
  if (!binary) return null;
  const agentsSrc = join(root, 'packages', 'app', 'src-tauri', 'src', 'pty', 'agents.rs');
  try {
    if (statSync(agentsSrc).mtimeMs > statSync(binary).mtimeMs) {
      console.error('[puppet-master] release binary is older than source; using tauri dev');
      return null;
    }
  } catch {
    /* keep release */
  }
  return binary;
}

async function resolveWorkerBinary(root: string): Promise<string | null> {
  const explicitBin = process.env.PUPPET_MASTER_BIN;
  if (explicitBin && existsSync(explicitBin)) return explicitBin;

  // A second `tauri dev` dies on Vite port 1420. Reuse the first worker's dev server.
  if (await portInUse(VITE_DEV_PORT)) {
    const debug = appBinary(root, 'debug');
    if (debug) {
      console.error('[puppet-master] vite already running; launching another worker against it');
      return debug;
    }
  }

  return releaseBinary(root);
}

function buildWorkerArgs(options: WorkerLaunchOptions): string[] {
  const args = [
    '--worker',
    '--agent-type',
    options.agentType ?? getDefaultTerminalAgentType(),
  ];
  if (options.cwd) args.push('--cwd', options.cwd);
  if (options.paneId) args.push('--pane-id', options.paneId);
  if (options.cols) args.push('--cols', String(options.cols));
  if (options.rows) args.push('--rows', String(options.rows));
  if (options.forceNew) args.push('--force-new');
  return args;
}

async function fetchJson(url: string, init?: RequestInit, timeoutMs = 3000): Promise<unknown> {
  const response = await fetch(url, { ...init, signal: AbortSignal.timeout(timeoutMs) });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`${response.status} ${text.slice(0, 200)}`);
  }
  return text ? JSON.parse(text) : null;
}

const BRIDGE_PROBE_SPAN = 8;

async function findLiveBridge(): Promise<string | null> {
  const tried = new Set<string>();
  const order: string[] = [];
  const enqueue = (host: string, port: number) => {
    if (!Number.isFinite(port) || port <= 0) return;
    const base = `http://${host}:${port}`;
    if (tried.has(base)) return;
    tried.add(base);
    order.push(base);
  };

  for (const hint of await readAllBridgePorts()) {
    enqueue(hint.host, hint.port);
  }
  for (let port = BRIDGE_HTTP_PORT_RANGE.min; port < BRIDGE_HTTP_PORT_RANGE.min + BRIDGE_PROBE_SPAN; port++) {
    enqueue('127.0.0.1', port);
  }

  for (const base of order) {
    try {
      const health = (await fetchJson(`${base}/health`, undefined, 800)) as { ok?: boolean };
      if (health?.ok) return base;
    } catch {
      /* next candidate */
    }
  }
  return null;
}

/**
 * A second `puppet_master_app` is its own pane registry. MCP list_panes follows
 * the latest bridge port file, so it only shows that process's one pane.
 * When a host is already up, spawn/detach against it instead — including `--new`.
 */
async function tryAttachToExistingHost(options: WorkerLaunchOptions): Promise<boolean> {
  const base = await findLiveBridge();
  if (!base) {
    console.error('[puppet-master] no existing worker host; starting a new process');
    return false;
  }

  const agentType = options.agentType ?? getDefaultTerminalAgentType();
  let paneId = options.forceNew ? undefined : options.paneId;
  try {
    if (!options.forceNew) {
      const panes = (await fetchJson(`${base}/panes`)) as PaneInfo[];
      if (paneId) {
        const live = panes.find((pane) => pane.id === paneId && pane.status !== 'error');
        if (!live) paneId = undefined;
      } else {
        paneId = findReusableWorkerPane(panes, agentType)?.id;
      }
    }

    if (!paneId) {
      const spawned = (await fetchJson(`${base}/panes`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          agent_type: agentType,
          ...(options.cwd ? { cwd: options.cwd } : {}),
          ...(options.cols ? { cols: options.cols } : {}),
          ...(options.rows ? { rows: options.rows } : {}),
          ...(options.paneId && options.forceNew ? { pane_id: options.paneId } : {}),
        }),
      })) as { pane_id?: string; snapshot?: { pane_id?: string } };
      paneId = spawned.pane_id ?? spawned.snapshot?.pane_id;
    }
    if (!paneId) {
      throw new Error('did not return a pane_id');
    }

    await fetchJson(`${base}/panes/${encodeURIComponent(paneId)}/detach`, { method: 'POST' });
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    throw new Error(`existing worker host ${base} refused attach: ${detail}`);
  }

  console.error(
    `[puppet-master] attached to existing host ${base} pane=${paneId} agent=${agentType} (list_panes now includes this pane)`,
  );
  return true;
}

/**
 * Launch a standalone worker terminal window (bridge + single pane, no main grid UI).
 */
export async function launchWorkerTerminal(options: WorkerLaunchOptions = {}): Promise<void> {
  if (await tryAttachToExistingHost(options)) {
    return;
  }

  const root = repoRoot();
  const workerArgs = buildWorkerArgs(options);
  const env = {
    ...process.env,
    PUPPET_MASTER_WORKER: '1',
    VITE_PUPPET_MASTER_WORKER: '1',
    PUPPET_MASTER_WORKER_AGENT: options.agentType ?? getDefaultTerminalAgentType(),
    ...(options.cwd ? { PUPPET_MASTER_WORKER_CWD: options.cwd } : {}),
    ...(options.paneId ? { PUPPET_MASTER_WORKER_PANE_ID: options.paneId } : {}),
  };

  // ponytail: only the release binary embeds the web UI unless Vite is already serving it.
  const binary = await resolveWorkerBinary(root);

  if (binary) {
    console.error(`[puppet-master] launching standalone worker: ${binary} ${workerArgs.join(' ')}`);
    const child = spawn(binary, workerArgs, {
      cwd: root,
      stdio: 'inherit',
      env,
      shell: false,
    });
    await waitForChild(child);
    return;
  }

  const npmArgs = ['run', 'tauri', '--', 'dev'];
  console.error(`[puppet-master] launching standalone worker (dev): npm ${npmArgs.join(' ')}`);
  const child = spawn('npm', npmArgs, {
    cwd: root,
    stdio: 'inherit',
    env,
    shell: process.platform === 'win32',
  });
  await waitForChild(child);
}

function waitForChild(child: ReturnType<typeof spawn>): Promise<void> {
  return new Promise((resolvePromise, reject) => {
    child.on('error', reject);
    child.on('exit', (code) => {
      if (code === 0 || code === null) resolvePromise();
      else reject(new Error(`worker process exited with code ${code}`));
    });
    process.on('SIGINT', () => child.kill('SIGINT'));
    process.on('SIGTERM', () => child.kill('SIGTERM'));
  });
}
