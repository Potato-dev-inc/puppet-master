/**
 * Freshness stamps for the generated Tauri resource folders (`resources/potato-brain`, `resources/potato-sandbox`).
 *
 * Those folders are gitignored build output. `ensure-tauri-resources.mjs` used to build them only when a file was
 * missing, so a dev checkout kept running the brain it had bundled the first day, long after `packages/potato-brain`
 * moved on (and a failed build left a stub that stayed forever). Now each bundle script writes a stamp: a sha256 of
 * everything that goes into the bundle. `ensure` rebuilds whenever the stamp is missing or differs. A content hash
 * (not file times) so a git checkout, a rebase or a copied worktree cannot fool it.
 */
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';

export const STAMP_NAME = '.source-hash';

const SKIP = new Set(['node_modules', '.DS_Store', 'Thumbs.db', STAMP_NAME]);

function walk(dir, out) {
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    if (SKIP.has(entry.name)) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else if (entry.isFile()) out.push(full);
  }
}

/**
 * sha256 of the files under each path (a file or a folder), by relative path and bytes, plus `extra` strings
 * (a tool version, say). A missing path counts as empty, so removing an input changes the hash too.
 */
export function hashInputs(root, paths, extra = []) {
  const hash = createHash('sha256');
  const files = [];
  for (const p of paths) {
    const full = join(root, p);
    if (!existsSync(full)) continue;
    if (statSync(full).isDirectory()) walk(full, files);
    else files.push(full);
  }
  for (const file of files) {
    hash.update(relative(root, file).split('\\').join('/'));
    hash.update('\0');
    hash.update(readFileSync(file));
    hash.update('\0');
  }
  for (const text of extra) hash.update(`extra:${text}\0`);
  return hash.digest('hex');
}

export function readStamp(outDir) {
  try {
    return readFileSync(join(outDir, STAMP_NAME), 'utf8').trim();
  } catch {
    return '';
  }
}

export function writeStamp(outDir, hash) {
  writeFileSync(join(outDir, STAMP_NAME), `${hash}\n`, 'utf8');
}

/** The esbuild version, so upgrading it rebuilds the bundle (empty when it is not installed). */
export function esbuildVersion(root) {
  try {
    return JSON.parse(readFileSync(join(root, 'node_modules', 'esbuild', 'package.json'), 'utf8')).version ?? '';
  } catch {
    return '';
  }
}

/** What goes into `resources/potato-brain`. */
export function brainInputsHash(root) {
  return hashInputs(
    root,
    ['packages/potato-brain/src', 'packages/potato-brain/persona', 'packages/potato-brain/package.json', 'scripts/bundle-potato-brain.mjs'],
    [esbuildVersion(root)],
  );
}

/** What goes into `resources/potato-sandbox`. */
export function sandboxInputsHash(root) {
  return hashInputs(root, [
    'packages/potato-sandbox/guest',
    'packages/potato-sandbox/recipes',
    'packages/potato-sandbox/viewer',
    'packages/potato-sandbox/LICENSE-APACHE-2.0',
    'scripts/bundle-potato-sandbox.mjs',
  ]);
}
