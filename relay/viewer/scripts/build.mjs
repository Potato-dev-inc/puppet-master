// Builds the viewer into dist/ (viewer.html, viewer.js, viewer.css). dist/ is committed, so the relay can
// include these files without a build step. Output is deterministic: run this after every source change.

import { copyFileSync, mkdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const src = join(root, 'src');
const dist = join(root, 'dist');

mkdirSync(dist, { recursive: true });

await build({
  entryPoints: [join(src, 'viewer.ts')],
  outfile: join(dist, 'viewer.js'),
  bundle: true,
  format: 'iife',
  platform: 'browser',
  target: 'es2020',
  minify: true,
  legalComments: 'none',
  logLevel: 'warning',
});

await build({
  entryPoints: [join(src, 'viewer.css')],
  outfile: join(dist, 'viewer.css'),
  minify: true,
  legalComments: 'none',
  logLevel: 'warning',
});

copyFileSync(join(src, 'viewer.html'), join(dist, 'viewer.html'));

for (const file of ['viewer.html', 'viewer.js', 'viewer.css']) {
  console.log(`${file} ${statSync(join(dist, file)).size} bytes`);
}
