#!/usr/bin/env node
/**
 * Ensure Tauri bundle resource globs exist before `tauri dev` / `tauri build`.
 * `pwa-dist` and `bin` are gitignored and produced by the production build scripts.
 */
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const pwaDist = join(root, 'packages/app/src-tauri/resources/pwa-dist');
const pwaAssets = join(pwaDist, 'assets');
const viteDist = join(root, 'packages/app/dist');
const binDir = join(root, 'packages/app/src-tauri/resources/bin');
const binName = process.platform === 'win32' ? 'cloudflared.exe' : 'cloudflared';
const cloudflaredSource = join(root, 'node_modules/cloudflared/bin', binName);

function runNodeScript(scriptName) {
  const script = join(root, 'scripts', scriptName);
  const result = spawnSync(process.execPath, [script], {
    cwd: root,
    stdio: 'inherit',
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function writeDevStubPwaDist() {
  mkdirSync(pwaAssets, { recursive: true });
  writeFileSync(
    join(pwaDist, 'index.html'),
    '<!doctype html><title>Puppet Master PWA</title><p>Dev stub — run npm run build -w @puppet-master/app for production assets.</p>\n',
    'utf-8',
  );
  writeFileSync(join(pwaAssets, 'dev-stub.txt'), 'dev\n', 'utf-8');
  console.error('[ensure-tauri-resources] wrote dev stub pwa-dist (vite serves the real UI in dev)');
}

function ensurePwaDist() {
  if (existsSync(join(pwaDist, 'index.html')) && existsSync(pwaAssets)) {
    return;
  }
  if (existsSync(viteDist)) {
    runNodeScript('stage-pwa-dist.mjs');
    return;
  }
  writeDevStubPwaDist();
}

function ensureCloudflaredBin() {
  const dest = join(binDir, binName);
  if (existsSync(dest)) {
    return;
  }
  if (existsSync(cloudflaredSource)) {
    runNodeScript('bundle-cloudflared.mjs');
    return;
  }
  mkdirSync(binDir, { recursive: true });
  writeFileSync(dest, '', 'utf-8');
  console.error('[ensure-tauri-resources] cloudflared missing — wrote empty bin stub for tauri dev');
}

export function ensureTauriResources() {
  ensurePwaDist();
  ensureCloudflaredBin();
}

const invokedDirectly =
  process.argv[1] &&
  fileURLToPath(import.meta.url).replace(/\\/g, '/') ===
    process.argv[1].replace(/\\/g, '/');

if (invokedDirectly) {
  ensureTauriResources();
}
