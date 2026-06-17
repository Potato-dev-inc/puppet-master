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

const __dirname = dirname(fileURLToPath(import.meta.url));

interface ParsedArgs {
  command: string;
  flags: Record<string, string | boolean>;
}

function parseArgs(argv: string[]): ParsedArgs {
  const [, , ...rest] = argv;
  const command = rest[0] && !rest[0].startsWith('-') ? rest[0] : 'gui';
  const flags: Record<string, string | boolean> = {};
  for (let i = command === 'gui' ? 0 : 1; i < rest.length; i++) {
    const a = rest[i];
    if (!a.startsWith('-')) continue;
    const key = a.replace(/^-+/, '');
    const next = rest[i + 1];
    if (next && !next.startsWith('-')) {
      flags[key] = next;
      i++;
    } else {
      flags[key] = true;
    }
  }
  return { command, flags };
}

function printHelp(): void {
  console.log(`puppet-master — multi-agent terminal orchestrator

Usage:
  puppet-master                 Launch the desktop GUI
  puppet-master --project PATH  Open with a preset cwd
  puppet-master mcp             Run stdio MCP server (GUI must be running)
  puppet-master version         Print version
  puppet-master --help          Show this help
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
  const { command, flags } = parseArgs(process.argv);

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
  const project = typeof flags.project === 'string' ? flags.project : undefined;
  await launchGui(project);
}

main().catch((err) => {
  console.error('[puppet-master] fatal:', err);
  process.exit(1);
});
