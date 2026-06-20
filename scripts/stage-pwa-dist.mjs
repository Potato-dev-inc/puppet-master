#!/usr/bin/env node
/**
 * Stage the Vite production build for the embedded mobile PWA HTTP server.
 */
import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const source = join(root, 'packages/app/dist');
const dest = join(root, 'packages/app/src-tauri/resources/pwa-dist');

if (!existsSync(source)) {
  console.error(`[stage-pwa-dist] missing ${source} — run vite build first`);
  process.exit(1);
}

rmSync(dest, { recursive: true, force: true });
mkdirSync(dirname(dest), { recursive: true });
cpSync(source, dest, { recursive: true });

console.error(`[stage-pwa-dist] staged ${source} → ${dest}`);
