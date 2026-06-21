import type { McpToolExecutor } from './mcp-tools';
import { sleep } from './ansi';
import { classifyPaneAttention } from './pane-attention';

/** Heuristics for agent permission / approval TUIs (Codex, Claude Code, etc.). */
const PERMISSION_HINTS = [
  /allow once/i,
  /allow always/i,
  /allow for session/i,
  /don't allow/i,
  /do not allow/i,
  /\bdeny\b/i,
  /\bapprove\b/i,
  /\bpermission\b/i,
  /\bproceed\b/i,
  /\bcontinue\b/i,
  /do you want to/i,
  /run this command/i,
  /execute this command/i,
  /trust this/i,
  /yes\/no/i,
  /\(y\/n\)/i,
  /are you sure/i,
  /\bconfirm\b/i,
];

export function isPermissionPrompt(buffer: string): boolean {
  if (!buffer.trim()) return false;
  if (/allow once[\s\S]{0,200}(deny|don't allow|do not allow)/i.test(buffer)) return true;
  if (/\b(allow once|allow always|allow for session)\b/i.test(buffer)) return true;
  let hits = 0;
  for (const re of PERMISSION_HINTS) {
    if (re.test(buffer)) hits++;
  }
  return hits >= 2;
}

export function wantsExplicitYes(buffer: string): boolean {
  return (
    /\b(proceed|continue)\b[\s\S]{0,240}(\(y\/n\)|yes\/no|\by\b\/\bn\b)/i.test(buffer) ||
    /do you want to[\s\S]{0,240}(\(y\/n\)|yes\/no|\by\b\/\bn\b)/i.test(buffer) ||
    /are you sure[\s\S]{0,240}(\(y\/n\)|yes\/no|\by\b\/\bn\b)/i.test(buffer) ||
    /\bconfirm[\s\S]{0,240}(\(y\/n\)|yes\/no|\by\b\/\bn\b)/i.test(buffer)
  );
}

/** Named TUI keys mapped to the bytes they should send to a PTY. */
export const KEY_SEQUENCES: Record<string, string> = {
  enter: '\r',
  return: '\r',
  escape: '\x1b',
  esc: '\x1b',
  tab: '\t',
  space: ' ',
  up: '\x1b[A',
  down: '\x1b[B',
  right: '\x1b[C',
  left: '\x1b[D',
  home: '\x1b[H',
  end: '\x1b[F',
  pageup: '\x1b[5~',
  pagedown: '\x1b[6~',
  y: 'y',
  n: 'n',
  yes: 'y',
  no: 'n',
};

/** Control-key sequences (Ctrl+C, Ctrl+D, etc.). */
export const CTRL_SEQUENCES: Record<string, string> = {
  c: '\x03',
  d: '\x04',
  z: '\x1a',
};

export type PressKeyResult =
  | { ok: true; key: string; bytes: number }
  | { ok: false; error: string };

/** Validate and send a named key to a pane. Does not append a newline. */
export async function pressKey(
  executor: McpToolExecutor,
  paneId: string,
  keyName: string,
): Promise<PressKeyResult> {
  const lower = keyName.trim().toLowerCase();
  if (lower.startsWith('ctrl+')) {
    const ch = lower.slice('ctrl+'.length);
    const seq = CTRL_SEQUENCES[ch];
    if (!seq) {
      return { ok: false, error: `unsupported ctrl key: ctrl+${ch}` };
    }
    await executor.writeInput(paneId, seq, false);
    return { ok: true, key: `ctrl+${ch}`, bytes: seq.length };
  }
  const seq = KEY_SEQUENCES[lower];
  if (!seq) {
    const known = [...Object.keys(KEY_SEQUENCES), ...Object.keys(CTRL_SEQUENCES).map((k) => `ctrl+${k}`)];
    return { ok: false, error: `unknown key "${keyName}". Known: ${known.join(', ')}` };
  }
  await executor.writeInput(paneId, seq, false);
  return { ok: true, key: lower, bytes: seq.length };
}

/** Press Enter only (no text). */
export async function pressEnter(executor: McpToolExecutor, paneId: string): Promise<void> {
  await executor.writeInput(paneId, '', true);
}

/**
 * Type text then Enter as two separate PTY writes (Codex/Ink TUIs need this).
 */
export async function typeAndSubmit(
  executor: McpToolExecutor,
  paneId: string,
  text: string,
): Promise<void> {
  const cleaned = text.replace(/[\r\n]+$/, '');
  if (cleaned.length > 0) {
    await executor.writeInput(paneId, cleaned, false);
    await sleep(60);
  }
  await pressEnter(executor, paneId);
  await sleep(120);
}

/**
 * Poll for permission prompts and press Enter to accept the default (usually "Allow once").
 * Returns a short log line for the MCP tool result.
 */
export async function autoApprovePermissions(
  executor: McpToolExecutor,
  paneId: string,
  maxMs = 5000,
): Promise<string> {
  const deadline = Date.now() + maxMs;
  let presses = 0;
  const maxPresses = 4;

  while (Date.now() < deadline && presses < maxPresses) {
    await sleep(350);
    const buf = await executor.readBuffer(paneId, 35);
    const attention = classifyPaneAttention(buf);
    if (attention.kind !== 'routine_permission') {
      if (presses > 0) return `auto-approved ${presses} permission prompt(s)`;
      return '';
    }
    if (wantsExplicitYes(buf)) {
      await typeAndSubmit(executor, paneId, 'y');
      presses++;
      await sleep(250);
      continue;
    }

    // Default selection is usually "Allow once" — Enter accepts it.
    // If the TUI needs explicit navigation, try left-arrow then Enter on retry.
    if (presses > 0) {
      await executor.writeInput(paneId, '\x1b[D', false); // left arrow
      await sleep(80);
    }
    await pressEnter(executor, paneId);
    presses++;
    await sleep(250);
  }

  if (presses > 0) return `auto-approved ${presses} permission prompt(s)`;
  return '';
}

/**
 * Single-shot check + approval. Reads the buffer once; if a permission prompt
 * is present, sends the appropriate approval keystroke. Designed for the
 * standby loop where we want a quick check without a long poll.
 *
 * Returns one of:
 *  - 'approved'        — a permission prompt was detected and an approval keystroke was sent
 *  - 'not-prompted'    — the buffer does not look like a permission prompt (caller should wake the LLM)
 *  - 'aborted'         — signal aborted mid-check
 */
export async function approvePermissionIfPresent(
  executor: McpToolExecutor,
  paneId: string,
  signal?: AbortSignal,
): Promise<'approved' | 'not-prompted' | 'aborted'> {
  if (signal?.aborted) return 'aborted';
  const buf = await executor.readBuffer(paneId, 40);
  if (signal?.aborted) return 'aborted';
  const attention = classifyPaneAttention(buf);
  if (attention.kind !== 'routine_permission') return 'not-prompted';

  if (wantsExplicitYes(buf)) {
    await typeAndSubmit(executor, paneId, 'y');
  } else {
    // Menu-style prompt (Allow once / Deny / …). Enter accepts the default
    // which is usually the safe "allow once" option.
    await pressEnter(executor, paneId);
  }
  await sleep(250);
  if (signal?.aborted) return 'aborted';
  return 'approved';
}
