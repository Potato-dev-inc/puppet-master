import type { McpToolExecutor } from './mcp-tools';
import { sleep } from './ansi';

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
  /run this command/i,
  /execute this command/i,
  /trust this/i,
  /yes\/no/i,
  /\(y\/n\)/i,
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
    if (!isPermissionPrompt(buf)) {
      if (presses > 0) return `auto-approved ${presses} permission prompt(s)`;
      return '';
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
