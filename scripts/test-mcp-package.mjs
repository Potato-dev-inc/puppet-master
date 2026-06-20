#!/usr/bin/env node
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const tmp = mkdtempSync(join(tmpdir(), 'puppet-master-mcp-package-'));
const portFile = join(tmp, 'bridge.port');
let bridge;
let mcp;
let bridgeErr = '';
let mcpErr = '';

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: options.capture ? ['ignore', 'pipe', 'pipe'] : 'inherit',
    encoding: 'utf8',
    ...options,
  });
  if (result.status !== 0) {
    const stderr = result.stderr ? `\n${result.stderr}` : '';
    throw new Error(`${command} ${args.join(' ')} failed${stderr}`);
  }
  return result;
}

function waitFor(predicate, label, timeoutMs = 5000) {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    const tick = () => {
      if (predicate()) {
        resolve();
        return;
      }
      if (Date.now() - start >= timeoutMs) {
        reject(new Error(`timed out waiting for ${label}`));
        return;
      }
      setTimeout(tick, 50);
    };
    tick();
  });
}

function send(proc, payload) {
  proc.stdin.write(`${JSON.stringify(payload)}\n`);
}

async function request(proc, state, method, params) {
  const id = ++state.id;
  send(proc, { jsonrpc: '2.0', id, method, params });
  const started = Date.now();
  while (Date.now() - started < 5000) {
    const found = state.messages.find((message) => message.id === id);
    if (found) {
      if (found.error) {
        throw new Error(`${method} returned error: ${JSON.stringify(found.error)}`);
      }
      return found.result;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`timed out waiting for ${method}`);
}

async function main() {
  run('npm', ['run', 'build:bridge']);
  run('npm', ['run', 'build:mcp']);

  const packResult = run(
    'npm',
    ['pack', '--workspace=@puppet-master/mcp', '--pack-destination', tmp, '--json'],
    { capture: true },
  );
  const [{ filename }] = JSON.parse(packResult.stdout);
  const tarball = join(tmp, filename);
  run('tar', ['-xzf', tarball, '-C', tmp], { cwd: tmp });
  const packedEntrypoint = join(tmp, 'package', 'dist', 'index.js');
  if (!existsSync(packedEntrypoint)) {
    throw new Error(`packed entrypoint missing: ${packedEntrypoint}`);
  }

  bridge = spawn(process.execPath, [join(repoRoot, 'packages/bridge/dist/server.js')], {
    cwd: repoRoot,
    env: { ...process.env, PUPPET_MASTER_BRIDGE_PORT_FILE: portFile },
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  bridge.stderr.on('data', (chunk) => {
    bridgeErr += chunk.toString();
  });
  await waitFor(() => existsSync(portFile), 'bridge port file');

  mcp = spawn(process.execPath, [packedEntrypoint], {
    cwd: repoRoot,
    env: { ...process.env, PUPPET_MASTER_BRIDGE_PORT_FILE: portFile },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  let pending = Buffer.alloc(0);
  const state = { id: 0, messages: [] };
  mcp.stderr.on('data', (chunk) => {
    mcpErr += chunk.toString();
  });
  mcp.on('exit', (code, signal) => {
    mcpErr += `[process exited code=${code} signal=${signal}]\n`;
  });
  mcp.stdout.on('data', (chunk) => {
    pending = Buffer.concat([pending, chunk]);
    while (true) {
      const newline = pending.indexOf('\n');
      if (newline === -1) break;
      const line = pending.slice(0, newline).toString('utf8').replace(/\r$/, '');
      pending = pending.slice(newline + 1);
      if (line) state.messages.push(JSON.parse(line));
    }
  });

  const initialized = await request(mcp, state, 'initialize', {
    protocolVersion: '2024-11-05',
    capabilities: {},
    clientInfo: { name: 'puppet-master-mcp-package-smoke', version: '0.0.0' },
  });
  send(mcp, { jsonrpc: '2.0', method: 'notifications/initialized' });
  const tools = await request(mcp, state, 'tools/list', {});
  const health = await request(mcp, state, 'tools/call', {
    name: 'bridge_health',
    arguments: {},
  });

  const toolNames = tools.tools.map((tool) => tool.name).sort();
  const healthText = health.content?.[0]?.text ?? '';
  const healthJson = JSON.parse(healthText);

  console.log(`packed: ${filename}`);
  console.log(`initialize: ${initialized.serverInfo.name} ${initialized.serverInfo.version}`);
  console.log(`tools: ${toolNames.join(', ')}`);
  console.log(`bridge_health: ${JSON.stringify(healthJson)}`);

  if (!toolNames.includes('bridge_health') || !toolNames.includes('spawn_agent')) {
    throw new Error(`expected MCP tools missing: ${toolNames.join(', ')}`);
  }
  if (healthJson.ok !== true) {
    throw new Error(`bridge_health returned non-ok response: ${healthText}`);
  }

  if (bridgeErr.includes('FAILED') || mcpErr.includes('Error:')) {
    console.error(bridgeErr);
    console.error(mcpErr);
    throw new Error('stderr contained a failure marker');
  }
}

try {
  await main();
} catch (err) {
  if (bridgeErr) console.error(`--- bridge stderr ---\n${bridgeErr}`);
  if (mcpErr) console.error(`--- mcp stderr ---\n${mcpErr}`);
  throw err;
} finally {
  if (mcp && !mcp.killed) mcp.kill('SIGTERM');
  if (bridge && !bridge.killed) bridge.kill('SIGTERM');
  rmSync(tmp, { recursive: true, force: true });
}
