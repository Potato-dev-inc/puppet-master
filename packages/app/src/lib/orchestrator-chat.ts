import type { OrchestratorChatEvent } from '@puppet-master/shared';

export interface ChatLine {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'tool' | 'error';
  text: string;
  streaming?: boolean;
}

export interface McpLogLine {
  id: string;
  tool: string;
  text: string;
  error?: boolean;
}

export function applyOrchestratorChatEvent(
  event: OrchestratorChatEvent,
  lines: ChatLine[],
): ChatLine[] {
  switch (event.type) {
    case 'user': {
      const id = `${event.message_id}-user`;
      if (lines.some((line) => line.id === id)) return lines;
      return [...lines, { id, role: 'user', text: event.text }];
    }
    case 'text': {
      const streamId = `${event.message_id}-stream`;
      const last = lines[lines.length - 1];
      if (last?.id === streamId) {
        return [...lines.slice(0, -1), { ...last, text: last.text + event.text, streaming: true }];
      }
      return [...lines, { id: streamId, role: 'assistant', text: event.text, streaming: true }];
    }
    case 'tool':
      return [
        ...lines,
        {
          id: `${event.message_id}-tool-${event.tool}-${lines.length}`,
          role: 'tool',
          text: event.error
            ? `${event.tool} — ${event.error}`
            : `${event.tool}${event.result ? ` → ${event.result.slice(0, 120)}` : ''}`,
        },
      ];
    case 'done':
      return lines.map((line) =>
        line.id === `${event.message_id}-stream` ? { ...line, streaming: false } : line,
      );
    case 'error':
      return [...lines, { id: `${event.message_id}-err`, role: 'error', text: event.error }];
    default:
      return lines;
  }
}

export function chatEventToMcpLog(event: OrchestratorChatEvent): McpLogLine | null {
  if (event.type !== 'tool') return null;
  return {
    id: `${event.message_id}-tool-${event.tool}`,
    tool: event.tool,
    text: event.result?.slice(0, 80) ?? '',
    error: Boolean(event.error),
  };
}
