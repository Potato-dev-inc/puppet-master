#!/usr/bin/env node
/**
 * Tauri beforeDevCommand entry: build workspace deps, start Vite, public dev tunnel.
 */
import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ensureWorkspaceBuilt } from './ensure-workspace-built.mjs';
import {
  clearDevInfo,
  isTunnelEnabled,
  startDevTunnel,
  tunnelDisabledHint,
  writeDevInfo,
} from './dev-tunnel.mjs';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const appDir = join(repoRoot, 'packages', 'app');
const portFile = join(repoRoot, 'puppet-master.bridge.port');
const VITE_PORT = 1420;
const VITE_URL = `http://127.0.0.1:${VITE_PORT}`;

ensureWorkspaceBuilt(repoRoot);

const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const vite = spawn(npm, ['exec', 'vite'], {
  cwd: appDir,
  stdio: 'inherit',
  env: process.env,
  shell: true,
});

/** @type {{ close: () => Promise<void> } | null} */
let devTunnel = null;

async function shutdown(signal) {
  if (devTunnel) {
    try {
      await devTunnel.close();
    } catch {
      /* ignore */
    }
    devTunnel = null;
  }
  await clearDevInfo();
  vite.kill(signal);
}

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => {
    void shutdown(signal);
  });
}

vite.on('exit', (code, signal) => {
  void (async () => {
    if (devTunnel) {
      try {
        await devTunnel.close();
      } catch {
        /* ignore */
      }
    }
    await clearDevInfo();
    if (signal) process.kill(process.pid, signal);
    else process.exit(code ?? 0);
  })();
});

void waitForVite().then(async () => {
  const bridge = readBridgeUrl();
  let tunnelUrl = null;
  let tunnelProvider = null;

  if (isTunnelEnabled()) {
    try {
      devTunnel = await startDevTunnel(VITE_PORT);
      tunnelUrl = devTunnel?.url ?? null;
      tunnelProvider = devTunnel?.provider ?? null;
    } catch (err) {
      console.error('[puppet-master] tunnel failed:', err instanceof Error ? err.message : err);
    }
  }

  const devInfo = await writeDevInfo({
    localUrl: VITE_URL,
    tunnelUrl,
    tunnelProvider,
    bridgeUrl: bridge,
  });

  console.error('');
  console.error('[puppet-master] dev stack ready');
  console.error(`  vite / PWA:     ${VITE_URL}`);
  console.error(`  bridge proxy:   ${VITE_URL}/bridge  (→ desktop bridge when app is up)`);
  if (bridge) console.error(`  bridge direct:  ${bridge}`);
  else console.error('  bridge direct:  starts with the Tauri window (embedded HTTP bridge)');

  if (devInfo.tunnelUrl) {
    console.error('');
    console.error('  📱 mobile PWA (any network):');
    console.error(`     ${devInfo.tunnelUrl}`);
    console.error(`     bridge: ${devInfo.bridgeProxyUrl}/health`);
    console.error('     open on your phone → Pair (QR) then Connect');
    if (devInfo.tunnelProvider) {
      console.error(`     via ${devInfo.tunnelProvider}`);
    }
  } else if (!isTunnelEnabled()) {
    console.error('');
    console.error(`  tunnel:         ${tunnelDisabledHint()}`);
  } else {
    console.error('');
    console.error('  tunnel:         failed to start — mobile access unavailable this session');
  }
  console.error('');
});

async function waitForVite(maxMs = 90_000) {
  const start = Date.now();
  while (Date.now() - start < maxMs) {
    try {
      const res = await fetch(`${VITE_URL}/`, { signal: AbortSignal.timeout(1500) });
      if (res.ok) return;
    } catch {
      /* retry */
    }
    await new Promise((r) => setTimeout(r, 500));
  }
}

function readBridgeUrl() {
  try {
    if (!existsSync(portFile)) return null;
    const raw = readFileSync(portFile, 'utf-8').trim();
    if (!raw) return null;
    if (raw.includes(':')) {
      const [host, port] = raw.split(':');
      return `http://${host || '127.0.0.1'}:${port}`;
    }
    return `http://127.0.0.1:${raw}`;
  } catch {
    return null;
  }
}
