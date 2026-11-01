// Agent Wizard, chat (ACP) part: the pure helpers behind the "Chat" step of the wizard dialog. They turn what the
// person typed (one item per line, no shell quoting) into an `[acp]` TOML block (docs/specs/agent-definitions/SPEC.md
// section 7.1), check it locally with plain-English messages, and read the backend's detection candidates. The
// backend checks the table again; these checks only save a round trip. Nothing here runs a program.

/** What the person typed on the ACP step. Lists are one item per line. */
export interface AcpFields {
  command: string;
  args: string;
  envAllow: string;
  resume: string;
}

/** The checked table, as it is sent and written. */
export interface AcpValue {
  command: string;
  args: string[];
  env_allow: string[];
  /** Absent when the person left it empty (no "Open in terminal"). */
  resume_command?: string[];
}

export interface AcpCandidate {
  command: string;
  args: string[];
  /** The help line that made the wizard think so. */
  evidence: string;
  confidence: 'high' | 'medium';
}

export interface AcpDetectResult {
  ok: boolean;
  command: string;
  candidates: AcpCandidate[];
  /** A `--stdio`-style hint from the help text, when the backend found one. */
  stdio_hint?: string | null;
  mentions_protocol?: boolean;
}

export type AcpErrors = Partial<Record<keyof AcpFields, string>>;

export const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
export const MAX_COMMAND_CHARS = 512;
export const MAX_ARGS = 64;

/** One item per line; blank lines are dropped. Items are kept as typed (no shell quoting, no trimming inside). */
export function splitLines(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.replace(/\r$/, ''))
    .filter((l) => l.trim() !== '');
}

export const emptyFields = (command = ''): AcpFields => ({ command, args: '', envAllow: '', resume: '' });

/** A candidate's values as editable fields. */
export function fieldsFromCandidate(c: AcpCandidate): AcpFields {
  return { command: c.command, args: c.args.join('\n'), envAllow: '', resume: '' };
}

/** Checks the fields. `value` is set only when there is no problem. */
export function validateAcp(f: AcpFields): { ok: true; value: AcpValue; errors: AcpErrors } | { ok: false; errors: AcpErrors; value?: undefined } {
  const errors: AcpErrors = {};
  const command = f.command.trim();
  if (!command) errors.command = 'Enter the program that speaks the chat protocol.';
  else if (/[\r\n]/.test(f.command.trim())) errors.command = 'The command must be on one line.';
  else if (command.length > MAX_COMMAND_CHARS) errors.command = `The command is too long (at most ${MAX_COMMAND_CHARS} characters).`;

  const args = splitLines(f.args);
  if (args.length > MAX_ARGS) errors.args = `Use at most ${MAX_ARGS} arguments.`;

  const env = splitLines(f.envAllow).map((s) => s.trim());
  const badEnv = env.find((n) => !ENV_NAME.test(n));
  if (badEnv !== undefined) {
    errors.envAllow = `"${badEnv}" is not a valid name. Use letters, digits and underscores, and do not start with a digit.`;
  }

  const resume = splitLines(f.resume);
  if (resume.length > 0 && !resume.some((a) => a.includes('{session}'))) {
    errors.resume = 'The resume command must contain {session}, the place where the session id goes.';
  } else if (resume.length > MAX_ARGS) {
    errors.resume = `Use at most ${MAX_ARGS} arguments.`;
  }

  if (Object.keys(errors).length > 0) return { ok: false, errors };
  const value: AcpValue = { command, args, env_allow: [...new Set(env)] };
  if (resume.length > 0) value.resume_command = resume;
  return { ok: true, value, errors };
}

// ---- TOML ---------------------------------------------------------------------------------------

/** A TOML basic string: quotes, backslashes and control characters escaped, so it reads back as the same text. */
export function tomlString(s: string): string {
  let out = '"';
  for (const ch of s) {
    const code = ch.codePointAt(0) ?? 0;
    if (ch === '\\') out += '\\\\';
    else if (ch === '"') out += '\\"';
    else if (ch === '\b') out += '\\b';
    else if (ch === '\t') out += '\\t';
    else if (ch === '\n') out += '\\n';
    else if (ch === '\f') out += '\\f';
    else if (ch === '\r') out += '\\r';
    else if (code < 0x20 || code === 0x7f) out += `\\u${code.toString(16).toUpperCase().padStart(4, '0')}`;
    else out += ch;
  }
  return out + '"';
}

const tomlArray = (items: string[]) => `[${items.map(tomlString).join(', ')}]`;

/** The `[acp]` table for a checked value. */
export function buildAcpBlock(v: AcpValue): string {
  const lines = ['[acp]', `command = ${tomlString(v.command)}`, `args = ${tomlArray(v.args)}`, `env_allow = ${tomlArray(v.env_allow)}`];
  if (v.resume_command && v.resume_command.length > 0) lines.push(`resume_command = ${tomlArray(v.resume_command)}`);
  return lines.join('\n') + '\n';
}

const HEADER = /^\s*\[\[?\s*([^\]]+?)\s*\]\]?\s*(#.*)?$/;

/** The text without any `[acp]` / `[acp.*]` table (a writer may add one by itself; the person's choice wins). */
export function stripAcp(toml: string): string {
  const out: string[] = [];
  let skipping = false;
  for (const line of toml.split('\n')) {
    const h = HEADER.exec(line.replace(/\r$/, ''));
    if (h) skipping = h[1] === 'acp' || h[1].startsWith('acp.');
    if (!skipping) out.push(line);
  }
  return out.join('\n');
}

/** The drafted definition with the person's `[acp]` table at the end. */
export function appendAcpBlock(toml: string, v: AcpValue): string {
  const base = stripAcp(toml).replace(/\s+$/, '');
  return `${base}\n\n${buildAcpBlock(v)}`;
}

// ---- display --------------------------------------------------------------------------------------

/** The table as plain text for the review step (rendered as React text, never as HTML). */
export function acpTableText(v: AcpValue): string {
  const list = (items: string[]) => (items.length ? items.join('  ') : '(none)');
  return [
    `Command:        ${v.command}`,
    `Arguments:      ${list(v.args)}`,
    `Allowed names:  ${list(v.env_allow)}`,
    `Resume command: ${v.resume_command && v.resume_command.length ? v.resume_command.join(' ') : '(none)'}`,
  ].join('\n');
}

/** The command line of a candidate, for the radio label. */
export function candidateLine(c: AcpCandidate): string {
  return [c.command, ...c.args].join(' ');
}

export function confidenceNote(c: AcpCandidate): string {
  return c.confidence === 'high' ? 'Looks like a good match.' : 'A possible match. Check it before you go on.';
}
