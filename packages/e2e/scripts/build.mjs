#!/usr/bin/env node
// Build the app (debug) and the fake agent once, into a cargo target folder of their own.
//   npm run e2e:build            E2E_CARGO_TARGET=D:/pm-cargo-e2e npm run e2e:build
//   flags: --skip-ui (do not rebuild the web UI), --skip-rust
// Never touches the running app; never builds in a checkout other than the one this file is in.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { REPO, SRC_TAURI, CARGO_TARGET, FAKE_TUI_TARGET, FAKE_TUI_MANIFEST, APP_EXE_DEFAULT, FAKE_TUI_EXE_DEFAULT, BUILD_DIR, BUILD_INFO } from '../src/paths.mjs';

const args = process.argv.slice(2);
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';
const t0 = Date.now();
const env = { ...process.env };
const cargoBin = path.join(process.env.USERPROFILE || process.env.HOME || '', '.cargo', 'bin');
if (fs.existsSync(cargoBin) && !(env.PATH || env.Path || '').toLowerCase().includes(cargoBin.toLowerCase())) {
  env[process.platform === 'win32' ? 'Path' : 'PATH'] = cargoBin + path.delimiter + (env.Path || env.PATH || '');
}

function run(label, cmd, cmdArgs, opts = {}) {
  const t = Date.now();
  console.log(`\n== ${label}`);
  const r = spawnSync(cmd, cmdArgs, { stdio: 'inherit', cwd: REPO, env, shell: process.platform === 'win32' && /\.cmd$/.test(cmd), ...opts });
  if (r.status !== 0) { console.error(`FAILED: ${label} (exit ${r.status})`); process.exit(r.status || 1); }
  console.log(`   ${label}: ${((Date.now() - t) / 1000).toFixed(1)} s`);
}

if (!fs.existsSync(path.join(REPO, 'node_modules'))) run('npm ci', npm, ['ci']);
if (!args.includes('--skip-ui')) {
  run('shared package', npm, ['run', 'build', '-w', 'packages/shared']);
  run('web UI (tsc + vite)', npx, ['--no-install', 'vite', 'build'], { cwd: path.join(REPO, 'packages', 'app') });
  run('stage UI', process.execPath, [path.join(REPO, 'scripts', 'stage-pwa-dist.mjs')]);
  run('resource placeholders', process.execPath, [path.join(REPO, 'scripts', 'ensure-tauri-resources.mjs')]);
}
if (!args.includes('--skip-rust')) {
  fs.mkdirSync(CARGO_TARGET, { recursive: true });
  run('app (cargo build, debug)', 'cargo', ['build', '--bin', 'puppet_master_app', '--features', 'dev-fake-tui'], { cwd: SRC_TAURI, env: { ...env, CARGO_TARGET_DIR: CARGO_TARGET } });
  run('fake agent (pm-fake-tui)', 'cargo', ['build', '--manifest-path', FAKE_TUI_MANIFEST], { env: { ...env, CARGO_TARGET_DIR: FAKE_TUI_TARGET } });
}
fs.mkdirSync(BUILD_DIR, { recursive: true });
let rev = ''; try { rev = spawnSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: REPO, encoding: 'utf8' }).stdout.trim(); } catch {}
const info = { appExe: APP_EXE_DEFAULT, fakeTuiExe: FAKE_TUI_EXE_DEFAULT, git: rev, builtAt: new Date().toISOString(), seconds: Math.round((Date.now() - t0) / 1000) };
fs.writeFileSync(BUILD_INFO, JSON.stringify(info, null, 1));
for (const f of [info.appExe, info.fakeTuiExe]) if (!fs.existsSync(f)) { console.error('missing build output: ' + f); process.exit(1); }
console.log(`\nbuilt in ${info.seconds} s\n  app:  ${info.appExe}\n  fake: ${info.fakeTuiExe}`);
