#!/usr/bin/env node
/**
 * Ensure Tauri bundle resource globs exist before `tauri dev` / `tauri build`.
 * `pwa-dist`, `bin` and `potato-brain` are gitignored and produced by the production build scripts.
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
const brainDir = join(root, 'packages/app/src-tauri/resources/potato-brain');
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

// The Potato Bot brain is a small esbuild bundle (tens of ms), so build the real one.
// If that cannot run, write a stub that exits with the "not configured" code, so tauri dev still starts.
function ensurePotatoBrain() {
  const brain = join(brainDir, 'brain.mjs');
  const persona = join(brainDir, 'persona');
  if (existsSync(brain) && existsSync(persona)) {
    return;
  }
  const result = spawnSync(process.execPath, [join(root, 'scripts', 'bundle-potato-brain.mjs')], {
    cwd: root,
    stdio: 'inherit',
  });
  if (result.status === 0) {
    return;
  }
  mkdirSync(persona, { recursive: true });
  const stub = [
    "console.error('potato-brain stub: bundle it with node scripts/bundle-potato-brain.mjs');",
    'process.exit(2);',
    '',
  ].join('\n');
  writeFileSync(brain, stub, 'utf-8');
  writeFileSync(join(persona, 'stub.txt'), 'stub\n', 'utf-8');
  console.error('[ensure-tauri-resources] potato-brain bundle failed — wrote a stub for tauri dev');
}

export function ensureTauriResources() {
  ensurePwaDist();
  ensureCloudflaredBin();
  ensurePotatoBrain();
}

const invokedDirectly =
  process.argv[1] &&
  fileURLToPath(import.meta.url).replace(/\\/g, '/') ===
    process.argv[1].replace(/\\/g, '/');

if (invokedDirectly) {
  ensureTauriResources();
}
