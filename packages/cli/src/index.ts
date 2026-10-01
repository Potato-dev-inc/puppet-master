#!/usr/bin/env node
/**
 * puppet-master — entry point for the Puppet Master CLI.
 *
 * Usage:
 *   npx puppet-master                 # launch GUI
 *   npx puppet-master --project PATH  # open with cwd preset
 *   npx puppet-master mcp             # run stdio MCP only (GUI must be running)
 *   npx puppet-master version         # print version
 *   npx puppet-master --help          # help
 */

import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { getDefaultTerminalAgentType } from '@puppet-master/shared';
import { launchWorkerTerminal } from './launch-worker.js';
import { runWatch } from './watch.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

interface ParsedArgs {
  command: string;
  positional: string[];
  flags: Record<string, string | boolean>;
}

const BOOLEAN_FLAGS = new Set(['help', 'h', 'version', 'v', 'force-new', 'new', 'force_new']);

function parseArgs(argv: string[]): ParsedArgs {
  const [, , ...rest] = argv;
  const command = rest[0] && !rest[0].startsWith('-') ? rest[0] : 'gui';
  const positional: string[] = [];
  const flags: Record<string, string | boolean> = {};
  const start = command === 'gui' ? 0 : 1;
  for (let i = start; i < rest.length; i++) {
    const a = rest[i];
    if (!a.startsWith('-')) {
      positional.push(a);
      continue;
    }
    const key = a.replace(/^-+/, '');
    if (BOOLEAN_FLAGS.has(key)) {
      flags[key] = true;
      continue;
    }
    const next = rest[i + 1];
    if (next && !next.startsWith('-')) {
      flags[key] = next;
      i++;
    } else {
      flags[key] = true;
    }
  }
  return { command, positional, flags };
}

function printHelp(): void {
  const defaultAgent = getDefaultTerminalAgentType();
  console.log(`puppet-master — multi-agent terminal orchestrator

Usage:
  puppet-master                 Launch the desktop GUI
  puppet-master --project PATH  Open with a preset cwd
  puppet-master worker [AGENT]  Open a standalone worker terminal (no main grid UI)
  puppet-master mcp             Run stdio MCP server (GUI or worker host must be running)
  puppet-master watch ID        Block until an operation reaches a terminal or needs-input state (JSON on stdout)
  puppet-master version         Print version
  puppet-master --help          Show this help

Worker examples:
  puppet-master worker                    # default shell (${defaultAgent})
  puppet-master worker powershell         # standalone PowerShell pane
  puppet-master worker powershell --new        # extra pane on the running host
  puppet-master worker powershell --force-new  # same as --new
`);
}

function printVersion(): void {
  try {
    const pkgPath = join(__dirname, '..', 'package.json');
    const pkg = JSON.parse(readFileSync(pkgPath, 'utf-8'));
    console.log(pkg.version);
  } catch {
    console.log('0.0.0');
  }
}

/**
 * Launch the Tauri desktop app. We spawn `npm run tauri --workspace=@puppet-master/app dev`
 * with the workspace root as cwd. The app will write a bridge port file we can read later.
 */
async function launchGui(projectPath?: string): Promise<void> {
  const repoRoot = resolve(__dirname, '..', '..', '..');
  const args = ['run', 'tauri', '--workspace=@puppet-master/app', 'dev'];
  if (projectPath) {
    args.push('--', '--project', projectPath);
  }
  console.error(`[puppet-master] launching GUI: npm ${args.join(' ')}`);
  const child = spawn('npm', args, {
    cwd: repoRoot,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
  child.on('exit', (code) => process.exit(code ?? 0));
  process.on('SIGINT', () => child.kill('SIGINT'));
  process.on('SIGTERM', () => child.kill('SIGTERM'));
}

/**
 * Run the MCP stdio server. Re-execs the @puppet-master/mcp package.
 */
async function runMcp(): Promise<void> {
  const candidates = [
    resolve(__dirname, '..', '..', 'mcp-server', 'dist', 'index.js'),
    resolve(__dirname, '..', '..', '..', 'packages', 'mcp-server', 'dist', 'index.js'),
  ];
  const target = candidates.find((p) => existsSync(p));
  if (!target) {
    console.error(
      '[puppet-master] @puppet-master/mcp is not built yet. Run: npm run build --workspace=@puppet-master/mcp',
    );
    process.exit(1);
  }
  const child = spawn(process.execPath, [target], { stdio: 'inherit' });
  child.on('exit', (code) => process.exit(code ?? 0));
}

async function main(): Promise<void> {
  const { command, positional, flags } = parseArgs(process.argv);

  if (flags.help || flags.h) {
    printHelp();
    return;
  }
  if (command === 'version' || flags.version || flags.v) {
    printVersion();
    return;
  }
  if (command === 'mcp') {
    await runMcp();
    return;
  }
  if (command === 'watch') {
    const ids = positional.length > 0 ? positional : [];
    if (ids.length === 0) {
      console.error('usage: puppet-master watch <operation_id|handle> [--project-path P] [--timeout-ms N] [--stream]');
      process.exit(3);
    }
    const projectPath =
      typeof flags['project-path'] === 'string'
        ? flags['project-path']
        : typeof flags.project === 'string'
          ? flags.project
          : undefined;
    const timeoutMs =
      typeof flags['timeout-ms'] === 'string' ? Number(flags['timeout-ms']) : 3_600_000;
    const stream = Boolean(flags.stream);
    const code = await runWatch({
      ids,
      projectPath,
      timeoutMs: Number.isFinite(timeoutMs) ? timeoutMs : 3_600_000,
      stream,
    });
    process.exit(code);
  }
  if (command === 'worker' || command === 'terminal') {
    const extra = positional.filter((value) => value !== 'new' && value !== 'force-new');
    const agentType =
      extra[0] ??
      (typeof flags['agent-type'] === 'string' ? flags['agent-type'] : undefined) ??
      (typeof flags.agent === 'string' ? flags.agent : undefined);
    await launchWorkerTerminal({
      agentType,
      cwd: typeof flags.cwd === 'string' ? flags.cwd : typeof flags.project === 'string' ? flags.project : undefined,
      paneId: typeof flags['pane-id'] === 'string' ? flags['pane-id'] : typeof flags.pane === 'string' ? flags.pane : undefined,
      cols: typeof flags.cols === 'string' ? Number(flags.cols) : undefined,
      rows: typeof flags.rows === 'string' ? Number(flags.rows) : undefined,
      forceNew:
        Boolean(flags['force-new'] || flags.new || flags.force_new) ||
        positional.some((value) => value === 'new' || value === 'force-new'),
    });
    return;
  }
  const project = typeof flags.project === 'string' ? flags.project : undefined;
  await launchGui(project);
}

main().catch((err) => {
  console.error('[puppet-master] fatal:', err);
  process.exit(1);
});
