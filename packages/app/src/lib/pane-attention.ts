export type PaneAttention =
  | { kind: 'none' }
  | { kind: 'routine_permission'; reason: string }
  | { kind: 'action_required'; reason: string }
  | { kind: 'report_ready'; reason: string }
  | { kind: 'terminal_error'; reason: string };

export function compactPaneText(text: string): string {
  return text
    .replace(/\u000f/g, '')
    .replace(/[^\S\r\n]+/g, ' ')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

export function hashPaneText(text: string): string {
  let hash = 2166136261;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16);
}

function looksLikeAmbiguousOpenCodePermission(text: string): boolean {
  return (
    /permission required/i.test(text) &&
    /allow once/i.test(text) &&
    /allow always/i.test(text) &&
    /\b(reject|deny|don't allow|do not allow)\b/i.test(text)
  );
}

function looksLikeSubstantiveQuestion(text: string): boolean {
  return (
    /\b(which|choose|pick|select)\b[\s\S]{0,120}\?/i.test(text) ||
    /\bwhich one\b/i.test(text) ||
    /\bpick one\b/i.test(text) ||
    /\bwhat would you like\b/i.test(text) ||
    /\bneed(s)? your (input|decision|confirmation)\b/i.test(text)
  );
}

function looksLikeTerminalError(text: string): boolean {
  return (
    /\b(command not found|no such file or directory|parse error near|permission denied)\b/i.test(text) ||
    /\bzsh:\d+:/i.test(text) ||
    /\bbash:\s.*:\s(command not found|no such file or directory)\b/i.test(text)
  );
}

function looksLikeReportReady(text: string): boolean {
  return (
    /\b(files created|files modified|summary|evidence|tests?)\s*:/i.test(text) &&
    /\b(done|complete|completed|pass|passed|failed|created|modified)\b/i.test(text)
  );
}

function looksLikePermissionPrompt(text: string): boolean {
  if (/allow once[\s\S]{0,200}(deny|don't allow|do not allow)/i.test(text)) return true;
  if (/\b(allow once|allow always|allow for session)\b/i.test(text)) return true;
  const hints = [
    /\b(permission|approve|proceed|continue)\b/i,
    /do you want to/i,
    /run this command/i,
    /execute this command/i,
    /trust this/i,
    /yes\/no/i,
    /\(y\/n\)/i,
    /are you sure/i,
    /\bconfirm\b/i,
  ];
  return hints.filter((re) => re.test(text)).length >= 2;
}

export function classifyPaneAttention(buffer: string, pane?: { agent_type?: string }): PaneAttention {
  const text = compactPaneText(buffer);
  if (!text) return { kind: 'none' };

  if (looksLikeAmbiguousOpenCodePermission(text)) {
    return {
      kind: 'action_required',
      reason: 'OpenCode permission prompt needs an explicit selection',
    };
  }

  if (looksLikePermissionPrompt(text)) {
    return {
      kind: 'routine_permission',
      reason: `${pane?.agent_type ?? 'agent'} permission/proceed prompt`,
    };
  }

  if (looksLikeSubstantiveQuestion(text)) {
    return { kind: 'action_required', reason: 'worker is asking a substantive question' };
  }

  if (looksLikeTerminalError(text)) {
    return { kind: 'terminal_error', reason: 'worker terminal shows an error that may need recovery' };
  }

  if (looksLikeReportReady(text)) {
    return { kind: 'report_ready', reason: 'worker appears to have produced a report' };
  }

  return { kind: 'none' };
}
