import { describe, expect, it } from 'vitest';
import { argvDiff, canReview, formatCommand, type DefinitionRow, type ProgramInfo } from './agentDefinitions';

const row = (over: Partial<DefinitionRow>): DefinitionRow => ({
  id: 'x',
  file: 'C:/a/x.toml',
  source: 'user',
  state: 'active',
  errors: [],
  fingerprint: 'sha256:1',
  shadowed_by: null,
  ...over,
});

describe('argvDiff', () => {
  it('marks added and removed arguments and keeps the rest', () => {
    const lines = argvDiff(['run', '--json', '<task>'], ['run', '--yolo', '--json', '<task>']);
    expect(lines.map((l) => `${l.kind}:${l.text}`)).toEqual([
      'same:run',
      'added:--yolo',
      'same:--json',
      'same:<task>',
    ]);
    const removed = argvDiff(['a', 'b'], ['a']);
    expect(removed.map((l) => `${l.kind}:${l.text}`)).toEqual(['same:a', 'removed:b']);
  });

  it('is all same for equal argv and all added for an empty approval', () => {
    expect(argvDiff(['a'], ['a']).every((l) => l.kind === 'same')).toBe(true);
    expect(argvDiff([], ['a', 'b']).every((l) => l.kind === 'added')).toBe(true);
  });
});

describe('canReview', () => {
  it('allows review for new, changed and binary-swapped definitions only', () => {
    expect(canReview(row({ state: 'needs_approval' }))).toBe(true);
    expect(canReview(row({ state: 'approval_stale' }))).toBe(true);
    expect(canReview(row({ state: 'active' }))).toBe(false);
    expect(canReview(row({ state: 'active', binary_changed: true }))).toBe(true);
    expect(canReview(row({ state: 'invalid', fingerprint: null }))).toBe(false);
    expect(canReview(row({ state: 'shadowed' }))).toBe(false);
  });
});

const FOUND: ProgramInfo = {
  name: 'kilo',
  path: 'C:\\Users\\someone\\AppData\\kilo.exe',
  display: '~\\AppData\\kilo.exe',
  found: true,
  note: null,
  tui_argv_has_program: false,
};

describe('formatCommand', () => {
  it('puts the program (user folder as ~) in front of the arguments', () => {
    const out = formatCommand(FOUND, ['--output-format', 'json', '--model', '<model>', '--print', '<task>'], 'headless');
    expect(out.text).toBe('~\\AppData\\kilo.exe --output-format json --model "<model>" --print "<task>"');
    expect(out.note).toBeNull();
  });

  it('puts plain quotes round a program path with a space, without doubling the slashes', () => {
    const out = formatCommand({ ...FOUND, display: 'C:\\Program Files\\kilo\\kilo.exe' }, ['run'], 'tui');
    expect(out.text).toBe('"C:\\Program Files\\kilo\\kilo.exe" run');
  });

  it('shows the name and a short note when the program is not installed', () => {
    const missing: ProgramInfo = { ...FOUND, path: null, display: 'kilo', found: false, note: 'not found on this PC' };
    const out = formatCommand(missing, ['run', '<task>'], 'headless');
    expect(out.text).toBe('kilo run "<task>"');
    expect(out.note).toBe('kilo: not found on this PC');
  });

  it('does not print a generic agent program twice in the visible command', () => {
    const generic: ProgramInfo = { ...FOUND, name: 'mycoder', display: '~\\bin\\mycoder.exe', tui_argv_has_program: true };
    expect(formatCommand(generic, ['mycoder', '--flag'], 'tui').text).toBe('~\\bin\\mycoder.exe --flag');
    expect(formatCommand(generic, ['mycoder'], 'tui').text).toBe('~\\bin\\mycoder.exe');
    // the headless command of a generic agent does not start with the program name
    expect(formatCommand(generic, ['run'], 'headless').text).toBe('~\\bin\\mycoder.exe run');
  });

  it('is the arguments alone when the backend sent no program, and null when there is no command', () => {
    expect(formatCommand(undefined, ['run', '<task>'], 'headless').text).toBe('run "<task>"');
    expect(formatCommand(FOUND, null, 'headless').text).toBeNull();
  });
});
