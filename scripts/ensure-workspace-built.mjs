#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

function collectFiles(dir, out = []) {
  for (const name of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, name.name);
    if (name.isDirectory()) collectFiles(full, out);
    else if (/\.(ts|tsx)$/.test(name.name)) out.push(full);
  }
  return out;
}

function needsBuild(distEntry, srcDir) {
  if (!existsSync(distEntry)) return true;
  const distMtime = statSync(distEntry).mtimeMs;
  if (!existsSync(srcDir)) return false;
  return collectFiles(srcDir).some((file) => statSync(file).mtimeMs > distMtime);
}

/**
 * Build workspace packages that the app imports at dev time.
 */
export function ensureWorkspaceBuilt(repoRoot) {
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const sharedDist = join(repoRoot, 'packages/shared/dist/index.js');
  const sharedSrc = join(repoRoot, 'packages/shared/src');

  if (!needsBuild(sharedDist, sharedSrc)) return;

  console.error('[dev] building @puppet-master/shared…');
  const result = spawnSync(npm, ['run', 'build', '-w', '@puppet-master/shared'], {
    cwd: repoRoot,
    stdio: 'inherit',
    shell: true,
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const invokedDirectly =
  process.argv[1] &&
  fileURLToPath(import.meta.url).replace(/\\/g, '/') ===
    process.argv[1].replace(/\\/g, '/');

if (invokedDirectly) {
  ensureWorkspaceBuilt(join(dirname(fileURLToPath(import.meta.url)), '..'));
}
