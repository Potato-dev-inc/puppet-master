// Message/log shapes of the Puppet Master sidebar plus the small pure reducers shared by its chat flows.

export interface UiLogEntry {
  id: string;
  ts: number;
  source: 'builtin' | 'external';
  tool: string;
  args: unknown;
  result?: string;
  error?: string;
}

export interface UiMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  text: string;
}

export const MAX_UI_LOG_ENTRIES = 500;

/** Newest-first log list capped at MAX_UI_LOG_ENTRIES. */
export function prependLogs(prev: UiLogEntry[], fresh: UiLogEntry[]): UiLogEntry[] {
  return [...fresh, ...prev].slice(0, MAX_UI_LOG_ENTRIES);
}

/** Log row for a tool the built-in orchestrator itself called. */
export function builtinToolLog(tool: string, args: unknown, result?: string, error?: string): UiLogEntry {
  return { id: crypto.randomUUID(), ts: Date.now(), source: 'builtin', tool, args, result, error };
}

/** Append streamed assistant text to the in-flight `streamId` message (or start it). */
export function appendStreamedText(messages: UiMessage[], streamId: string, text: string): UiMessage[] {
  const last = messages[messages.length - 1];
  if (last && last.role === 'assistant' && last.id === streamId) {
    return [...messages.slice(0, -1), { ...last, text: last.text + text }];
  }
  return [...messages, { id: streamId, role: 'assistant', text }];
}
