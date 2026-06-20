#!/usr/bin/env node
/**
 * Copy the platform cloudflared binary into Tauri resources for production tunnels.
 */
import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const binName = process.platform === 'win32' ? 'cloudflared.exe' : 'cloudflared';
const source = join(root, 'node_modules/cloudflared/bin', binName);
const destDir = join(root, 'packages/app/src-tauri/resources/bin');
const dest = join(destDir, binName);

if (!existsSync(source)) {
  console.error(`[bundle-cloudflared] missing ${source} — run npm install`);
  process.exit(1);
}

mkdirSync(destDir, { recursive: true });
copyFileSync(source, dest);
if (process.platform !== 'win32') {
  chmodSync(dest, 0o755);
}

console.error(`[bundle-cloudflared] wrote ${dest}`);
