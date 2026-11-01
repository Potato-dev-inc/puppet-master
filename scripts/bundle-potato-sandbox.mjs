#!/usr/bin/env node
/**
 * Stage the Potato Bot sandbox files (packages/potato-sandbox) for the Tauri app bundle.
 *
 *   packages/potato-sandbox/guest/    -> packages/app/src-tauri/resources/potato-sandbox/guest/     (copied into the distro)
 *   packages/potato-sandbox/recipes/  -> packages/app/src-tauri/resources/potato-sandbox/recipes/   (the wizard's menu)
 *   packages/potato-sandbox/viewer/   -> packages/app/src-tauri/resources/potato-sandbox/viewer/    (embed.html + novnc)
 *
 * Same idea as bundle-potato-brain.mjs: the output is generated and gitignored; scripts/ensure-tauri-resources.mjs
 * runs this when it is missing. It is only a copy (no build step), so it cannot fail for a missing tool.
 */
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync } from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const pkg = join(root, 'packages/potato-sandbox');
const outDir = join(root, 'packages/app/src-tauri/resources/potato-sandbox');
const PARTS = ['guest', 'recipes', 'viewer'];

for (const part of PARTS) {
  if (!existsSync(join(pkg, part))) {
    console.error(`[bundle-potato-sandbox] missing ${join(pkg, part)}`);
    process.exit(1);
  }
}
if (!existsSync(join(pkg, 'viewer', 'embed.html'))) {
  console.error('[bundle-potato-sandbox] viewer/embed.html is missing');
  process.exit(1);
}

mkdirSync(outDir, { recursive: true });
for (const part of PARTS) {
  const dest = join(outDir, part);
  // Only ever clear a folder this script owns.
  if (existsSync(dest) && dest.startsWith(outDir + sep)) rmSync(dest, { recursive: true, force: true });
  cpSync(join(pkg, part), dest, { recursive: true });
}
cpSync(join(pkg, 'LICENSE-APACHE-2.0'), join(outDir, 'LICENSE-APACHE-2.0'));

function count(dir) {
  let n = 0;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    n += entry.isDirectory() ? count(join(dir, entry.name)) : statSync(join(dir, entry.name)).isFile() ? 1 : 0;
  }
  return n;
}
console.error(`[bundle-potato-sandbox] wrote ${outDir} (${count(outDir)} files: ${PARTS.join(', ')})`);
