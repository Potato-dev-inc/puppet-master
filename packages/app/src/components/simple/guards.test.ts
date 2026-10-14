// The two guards of Simple mode, seen from the UI side:
//  - a broad folder is refused (the list of cases is shared with the Rust guard: folder-cases.json);
//  - a copy is always used (the command the UI calls has no way to ask for anything else).
// The authority is Rust (`plain_mode/tests.rs` runs the same cases); this file keeps the UI copy
// honest and checks what the UI sends.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import cases from './folder-cases.json';
import { folderVerdict, normalize, refusalCode, type KnownDirs, type Verdict } from './folder-guard';
import { refusalText } from './MakeSomething';
import { errorCode, simpleApi } from './simple-api';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const known = cases.known as KnownDirs;

describe('a broad folder is refused', () => {
  it.each(cases.cases.map((c) => [JSON.stringify(c.path), c.path, c.verdict]))('%s', (_label, path, verdict) => {
    expect(folderVerdict(path as string, known)).toBe(verdict as Verdict);
  });

  it('refuses the folders the design names, and only those', () => {
    const refused = cases.cases.filter((c) => c.verdict !== 'ok').map((c) => c.verdict);
    for (const kind of ['home', 'desktop', 'documents', 'downloads', 'onedrive', 'drive_root', 'system', 'above_home', 'projects_root']) {
      expect(refused).toContain(kind);
    }
    // Every refusal has a code the UI can turn into plain words; a good folder has none.
    for (const c of cases.cases) {
      expect(refusalCode(c.verdict as Verdict) === null).toBe(c.verdict === 'ok');
    }
  });

  it('ignores case, trailing slashes and the long-path prefix', () => {
    expect(normalize('\\\\?\\C:\\Users\\Ana\\')).toBe('c:/users/ana');
    expect(folderVerdict('C:/USERS/ANA/', known)).toBe('home');
  });

  it('the Rust guard reads this same list', () => {
    const rust = readFileSync(join(__dirname, '../../../src-tauri/src/plain_mode/tests.rs'), 'utf8');
    expect(rust).toContain('folder-cases.json');
  });
});

describe('a copy is always used', () => {
  beforeEach(() => invoke.mockReset());

  it('the start command carries no setting that could turn the copy off', async () => {
    invoke.mockResolvedValue({ pane_id: 'p1', handle: 'h1', folder: 'C:\\x' });
    await simpleApi.start({ name: 'Quiz', prompt: 'Make a quiz.', agent: 'claude' });
    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, args] = invoke.mock.calls[0];
    expect(command).toBe('plain_start');
    const sent = JSON.stringify(args);
    for (const word of ['isolation', 'worktree', 'headless', 'allow_shell', 'trust_workspace', 'dangerously']) {
      expect(sent).not.toContain(word);
    }
    expect(Object.keys((args as { input: object }).input).sort()).toEqual(['agent', 'name', 'prompt']);
  });

  it('the UI never calls the cockpit command that starts a pane in the main folder', () => {
    const files = ['MakeSomething.tsx', 'SimpleProject.tsx', 'SimpleHome.tsx', 'SimpleCockpit.tsx', 'simple-api.ts'];
    for (const file of files) {
      const text = readFileSync(join(__dirname, file), 'utf8');
      expect(text, file).not.toMatch(/newPane|new_pane|spawnPane|newAgent/);
    }
  });

  it('the Rust side sets the copy itself', () => {
    const rust = readFileSync(join(__dirname, '../../../src-tauri/src/plain_mode/mod.rs'), 'utf8');
    expect(rust).toContain('"isolation": "worktree"');
  });

  it('a refusal from Rust becomes plain words, never a code', () => {
    for (const code of ['FOLDER_TOO_BROAD', 'FOLDER_TOO_BIG', 'FOLDER_MISSING', 'FOLDER_NEEDS_SETUP', 'ANYTHING_ELSE']) {
      expect(refusalText(code)).not.toMatch(/[A-Z]{3,}_[A-Z]/);
    }
    expect(errorCode(new Error('FOLDER_TOO_BROAD: C:\\x is not a folder'))).toBe('FOLDER_TOO_BROAD');
    expect(errorCode('nothing here')).toBeNull();
  });
});
