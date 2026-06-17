/** Strip ANSI/VT escape sequences from PTY scrollback. */
// eslint-disable-next-line no-control-regex
const ANSI_RE = /\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])/g;

export function stripAnsi(text: string): string {
  const lines = text
    .replace(ANSI_RE, '')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

  return lines
    .filter((line, index) => index === 0 || line !== lines[index - 1])
    .join('\n');
}

/** Keep the tail of buffer text that's useful for an LLM. */
export function summarizeBuffer(text: string, maxChars = 3500): string {
  const clean = stripAnsi(text);
  if (clean.length <= maxChars) return clean;
  return `…(truncated)\n${clean.slice(-maxChars)}`;
}

const TUI_AGENTS = new Set(['claude', 'codex', 'opencode']);

export function isTuiAgent(agentType: string): boolean {
  return TUI_AGENTS.has(agentType);
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
