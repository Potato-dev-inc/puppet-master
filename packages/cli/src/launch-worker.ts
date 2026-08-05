import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { getDefaultTerminalAgentType } from '@puppet-master/shared';

const __dirname = dirname(fileURLToPath(import.meta.url));

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

function releaseBinary(root: string): string | null {
  const base = join(root, 'packages', 'app', 'src-tauri', 'target', 'release');
  const binary =
    process.platform === 'win32'
      ? join(base, 'puppet_master_app.exe')
      : join(base, 'puppet_master_app');
  return existsSync(binary) ? binary : null;
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

/**
 * Launch a standalone worker terminal window (bridge + single pane, no main grid UI).
 */
export async function launchWorkerTerminal(options: WorkerLaunchOptions = {}): Promise<void> {
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

  // ponytail: only the release binary embeds/bundles the web UI; debug cargo builds open a blank window.
  const explicitBin = process.env.PUPPET_MASTER_BIN;
  const binary =
    explicitBin && existsSync(explicitBin) ? explicitBin : releaseBinary(root);

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
