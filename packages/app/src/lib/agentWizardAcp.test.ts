import { describe, expect, it } from 'vitest';
import {
  acpTableText,
  appendAcpBlock,
  buildAcpBlock,
  emptyFields,
  splitLines,
  stripAcp,
  tomlString,
  validateAcp,
  type AcpValue,
} from './agentWizardAcp';

/** A reader for the small TOML subset the block uses: `key = "string"` and `key = ["a", "b"]` under `[acp]`. */
function readAcp(toml: string): Record<string, string | string[]> {
  const out: Record<string, string | string[]> = {};
  let inAcp = false;
  const map: Record<string, string> = { b: '\b', t: '\t', n: '\n', f: '\f', r: '\r', '"': '"', '\\': '\\' };
  const str = (src: string, at: number): [string, number] => {
    let i = at + 1;
    let v = '';
    while (src[i] !== '"') {
      if (src[i] === '\\') {
        const n = src[i + 1];
        if (n === 'u') {
          v += String.fromCharCode(parseInt(src.slice(i + 2, i + 6), 16));
          i += 6;
        } else {
          v += map[n];
          i += 2;
        }
      } else {
        v += src[i++];
      }
    }
    return [v, i + 1];
  };
  for (const line of toml.split('\n')) {
    if (/^\s*\[/.test(line)) inAcp = line.trim() === '[acp]';
    else if (inAcp && line.includes(' = ')) {
      const key = line.slice(0, line.indexOf(' = '));
      const rest = line.slice(line.indexOf(' = ') + 3);
      if (rest.startsWith('[')) {
        const items: string[] = [];
        let i = 1;
        while (rest[i] !== ']') {
          if (rest[i] === '"') {
            const [v, next] = str(rest, i);
            items.push(v);
            i = next;
          } else i += 1;
        }
        out[key] = items;
      } else out[key] = str(rest, 0)[0];
    }
  }
  return out;
}

const NASTY = ['a "quoted" arg', 'back\\slash C:\\x\\y', 'tab\there', 'line\nbreak', 'bell\u0007', '{session}', 'unicode \u00e9 \u{1F600}', '#not a comment'];

describe('validateAcp', () => {
  it('needs a command', () => {
    const r = validateAcp(emptyFields(''));
    expect(r.ok).toBe(false);
    expect(r.errors.command).toMatch(/Enter the program/);
  });

  it('refuses a command with a line break', () => {
    expect(validateAcp({ ...emptyFields('a\nb') }).errors.command).toMatch(/one line/);
  });

  it('checks environment names', () => {
    expect(validateAcp({ ...emptyFields('x'), envAllow: 'GOOD_1\n9bad' }).errors.envAllow).toContain('"9bad"');
    expect(validateAcp({ ...emptyFields('x'), envAllow: 'has space' }).ok).toBe(false);
    expect(validateAcp({ ...emptyFields('x'), envAllow: '_ok\nOK2' }).ok).toBe(true);
  });

  it('wants {session} in the resume command', () => {
    expect(validateAcp({ ...emptyFields('x'), resume: '--resume\nabc' }).errors.resume).toMatch(/\{session\}/);
    const ok = validateAcp({ ...emptyFields('x'), resume: '--resume\n{session}' });
    expect(ok.ok && ok.value.resume_command).toEqual(['--resume', '{session}']);
  });

  it('drops blank lines, trims names and the command, and de-duplicates names', () => {
    const r = validateAcp({ command: ' kilo ', args: 'acp\n\n  --x  \n', envAllow: ' A \nA\n', resume: '' });
    expect(r.ok && r.value).toEqual({ command: 'kilo', args: ['acp', '  --x  '], env_allow: ['A'] });
  });

  it('splits lines of both kinds', () => {
    expect(splitLines('a\r\nb\n\n c')).toEqual(['a', 'b', ' c']);
  });
});

describe('the [acp] block', () => {
  it('reads back as the same values, however awkward the text', () => {
    const v: AcpValue = { command: 'my "cmd"', args: NASTY, env_allow: ['HOME', 'API_KEY'], resume_command: ['--resume', '{session}', 'x\\y'] };
    const read = readAcp(buildAcpBlock(v));
    expect(read.command).toBe(v.command);
    expect(read.args).toEqual(v.args);
    expect(read.env_allow).toEqual(v.env_allow);
    expect(read.resume_command).toEqual(v.resume_command);
  });

  it('omits an empty resume command and keeps empty lists', () => {
    const block = buildAcpBlock({ command: 'k', args: [], env_allow: [] });
    expect(block).toBe('[acp]\ncommand = "k"\nargs = []\nenv_allow = []\n');
  });

  it('escapes controls as basic-string escapes', () => {
    expect(tomlString('a\u0001b\u007f')).toBe('"a\\u0001b\\u007F"');
    expect(tomlString('q"\\')).toBe('"q\\"\\\\"');
  });

  it('appends after the draft with one blank line and a trailing newline', () => {
    const out = appendAcpBlock('schema_version = 1\n[agent]\nid = "x"\n\n\n', { command: 'k', args: ['acp'], env_allow: [] });
    expect(out).toBe('schema_version = 1\n[agent]\nid = "x"\n\n[acp]\ncommand = "k"\nargs = ["acp"]\nenv_allow = []\n');
  });

  it('replaces an [acp] table the writer added itself, with its sub-tables, and keeps the others', () => {
    const draft = 'a = 1\n[acp]\ncommand = "old"\n[acp.quirks]\nx = true\n[tui]\nkeep = 1\n';
    const out = appendAcpBlock(draft, { command: 'new', args: [], env_allow: [] });
    expect(stripAcp(draft)).toBe('a = 1\n[tui]\nkeep = 1\n');
    expect(out).toContain('[tui]\nkeep = 1');
    expect(out).not.toContain('old');
    expect(out.match(/\[acp\]/g)).toHaveLength(1);
  });

  it('shows the table as text', () => {
    expect(acpTableText({ command: 'k', args: ['acp'], env_allow: [], resume_command: ['--r', '{session}'] })).toBe(
      'Command:        k\nArguments:      acp\nAllowed names:  (none)\nResume command: --r {session}',
    );
  });
});
