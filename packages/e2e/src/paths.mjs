// Where things live. Every path is derived from the repo root or from an env var; nothing is hard-coded to a person.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const E2E_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const REPO = path.resolve(E2E_DIR, '..', '..');
export const TESTS_DIR = path.join(E2E_DIR, 'tests');
export const BUILD_DIR = path.join(E2E_DIR, '.build');            // gitignored
export const RESULTS_DIR = path.resolve(process.env.E2E_RESULTS_DIR || path.join(REPO, 'e2e-results'));   // gitignored
export const SRC_TAURI = path.join(REPO, 'packages', 'app', 'src-tauri');
export const FAKE_TUI_MANIFEST = path.join(REPO, 'tools', 'pm-fake-tui', 'Cargo.toml');
export const AGENT_FIXTURES = path.join(E2E_DIR, 'fixtures', 'agents');

/** Cargo target dir for the e2e build: E2E_CARGO_TARGET, else under the ignored .build folder. */
export const CARGO_TARGET = path.resolve(process.env.E2E_CARGO_TARGET || path.join(BUILD_DIR, 'cargo'));
export const exeName = (n) => (process.platform === 'win32' ? `${n}.exe` : n);
export const APP_EXE_DEFAULT = path.join(CARGO_TARGET, 'debug', exeName('puppet_master_app'));
export const FAKE_TUI_TARGET = path.join(CARGO_TARGET, 'fake-tui');
export const FAKE_TUI_EXE_DEFAULT = path.join(FAKE_TUI_TARGET, 'debug', exeName('pm-fake-tui'));
export const BUILD_INFO = path.join(BUILD_DIR, 'build.json');

export function readBuildInfo() {
  try { return JSON.parse(fs.readFileSync(BUILD_INFO, 'utf8')); } catch { return null; }
}
export function appExe() { return path.resolve(process.env.E2E_APP_EXE || readBuildInfo()?.appExe || APP_EXE_DEFAULT); }
export function fakeTuiExe() { return path.resolve(process.env.E2E_FAKE_TUI_EXE || readBuildInfo()?.fakeTuiExe || FAKE_TUI_EXE_DEFAULT); }

/** A short random run id such as 20261030-141502-ab3f. */
export function newRunId() {
  const d = new Date(); const p = (n) => String(n).padStart(2, '0');
  const stamp = `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
  return `${stamp}-${Math.random().toString(16).slice(2, 6)}`;
}
export const tmpBase = () => path.join(os.tmpdir(), 'pm-e2e');
