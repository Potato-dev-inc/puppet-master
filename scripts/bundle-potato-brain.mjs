#!/usr/bin/env node
/**
 * Bundle the Potato Bot brain (packages/potato-brain) for the Tauri app bundle.
 *
 *   src/pm.ts  ->  packages/app/src-tauri/resources/potato-brain/brain.mjs   (ESM, node20)
 *   persona/   ->  packages/app/src-tauri/resources/potato-brain/persona/
 *
 * The app starts `node brain.mjs` and points FISHD_PERSONA_DIR at the persona
 * folder. Phase 1 has no chat-app adapter in the bundle, and so no
 * discord.js; the script fails if one ever slips in.
 *
 * The output is generated and gitignored, like resources/pwa-dist and resources/bin.
 * `scripts/ensure-tauri-resources.mjs` builds it when it is missing.
 */
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { dirname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import esbuild from 'esbuild';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const pkg = resolve(root, 'packages/potato-brain');
const outDir = resolve(root, 'packages/app/src-tauri/resources/potato-brain');
const outfile = resolve(outDir, 'brain.mjs');
const personaOut = resolve(outDir, 'persona');

mkdirSync(outDir, { recursive: true });

await esbuild.build({
  entryPoints: [resolve(pkg, 'src/pm.ts')],
  bundle: true,
  platform: 'node',
  target: 'node20',
  format: 'esm',
  outfile,
  legalComments: 'none',
  logLevel: 'info',
});

const bundled = readFileSync(outfile, 'utf8');
if (/discord/i.test(bundled)) {
  console.error('[bundle-potato-brain] the bundle mentions discord: phase 1 ships no chat-app adapter');
  process.exit(1);
}

// Only ever clear the folder this script owns.
if (existsSync(personaOut) && personaOut.startsWith(outDir + sep)) {
  rmSync(personaOut, { recursive: true, force: true });
}
cpSync(resolve(pkg, 'persona'), personaOut, { recursive: true });

const kb = Math.round(statSync(outfile).size / 1024);
console.error(`[bundle-potato-brain] wrote ${outfile} (${kb} KB) and persona/`);
