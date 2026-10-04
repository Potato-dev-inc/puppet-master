// Chat view for the API orchestrator backend (transcript, MCP tool log, composer).
import { useCallback, useEffect, useRef, useState } from 'react';
import type { OrchestratorChatEvent } from '@puppet-master/shared';
import type { BridgeClient } from '../../lib/bridge';
import {
  applyOrchestratorChatEvent,
  chatEventToMcpLog,
  type ChatLine,
  type McpLogLine,
} from '../../lib/orchestrator-chat';

export function ChatTab({
  bridge,
  chatEvents,
  bridgeReady,
}: {
  bridge: BridgeClient;
  chatEvents: OrchestratorChatEvent[];
  bridgeReady: boolean;
}) {
  const [lines, setLines] = useState<ChatLine[]>([]);
  const [logs, setLogs] = useState<McpLogLine[]>([]);
  const [draft, setDraft] = useState('');
  const [sending, setSending] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);
  const seenCount = useRef(0);

  useEffect(() => {
    const newEvents = chatEvents.slice(seenCount.current);
    if (newEvents.length === 0) return;
    seenCount.current = chatEvents.length;
    setLines((prev) => {
      let next = prev;
      for (const ev of newEvents) next = applyOrchestratorChatEvent(ev, next);
      return next;
    });
    setLogs((prev) => {
      const fresh = newEvents
        .map(chatEventToMcpLog)
        .filter((entry): entry is McpLogLine => entry !== null);
      return [...fresh, ...prev].slice(0, 50);
    });
  }, [chatEvents]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [lines]);

  const sendMessage = useCallback(async () => {
    const text = draft.trim();
    if (!text || sending || !bridgeReady) return;
    const messageId = crypto.randomUUID();
    setDraft('');
    setSending(true);
    setLines((prev) => applyOrchestratorChatEvent(
      { type: 'user', message_id: messageId, text },
      prev,
    ));
    try {
      await bridge.postOrchestratorMessage(text, messageId);
    } catch (e) {
      setLines((prev) => [
        ...prev,
        {
          id: `${messageId}-send-err`,
          role: 'error',
          text: e instanceof Error ? e.message : String(e),
        },
      ]);
    } finally {
      setSending(false);
    }
  }, [bridge, bridgeReady, draft, sending]);

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex-1 min-h-0 overflow-y-auto p-3 space-y-2">
        {!bridgeReady && (
          <div className="text-xs text-pm-muted text-center py-4">Connecting to desktop…</div>
        )}
        {lines.length === 0 && bridgeReady && (
          <div className="text-xs text-pm-muted">
            Ask the Puppet Master to coordinate your panes — same as the desktop sidebar.
          </div>
        )}
        {lines.map((line) => (
          <div
            key={line.id}
            className={`text-xs whitespace-pre-wrap ${
              line.role === 'user'
                ? 'text-zinc-100'
                : line.role === 'assistant'
                ? 'text-zinc-200'
                : line.role === 'tool'
                ? 'text-pm-muted font-mono'
                : line.role === 'error' || line.role === 'system'
                ? 'text-pm-warn'
                : 'text-pm-muted'
            }`}
          >
            <span className="text-pm-muted mr-1">[{line.role}]</span>
            {line.role === 'assistant' && line.streaming ? `${line.text}▋` : line.text}
          </div>
        ))}
        <div ref={bottomRef} />
      </div>

      <div className="border-t border-pm-border max-h-28 overflow-auto shrink-0">
        <div className="px-3 py-1 text-[10px] uppercase tracking-wide text-pm-muted border-b border-pm-border bg-pm-bg/40">
          MCP log
        </div>
        {logs.length === 0 ? (
          <div className="px-3 py-1 text-[10px] text-pm-muted">No tool calls yet.</div>
        ) : (
          logs.slice(0, 20).map((log) => (
            <div key={log.id} className="px-3 py-0.5 text-[10px] font-mono">
              <span className={log.error ? 'text-pm-err' : 'text-pm-accent'}>{log.tool}</span>
              {log.text && <span className="text-pm-muted"> → {log.text}</span>}
            </div>
          ))
        )}
      </div>

      <div className="border-t border-pm-border p-2 shrink-0">
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              void sendMessage();
            }
          }}
          rows={2}
          placeholder="Ask the Puppet Master… (Ctrl+Enter)"
          className="w-full text-xs bg-pm-bg border border-pm-border rounded p-2 resize-none text-zinc-100 focus:outline-none focus:border-pm-accent"
          disabled={sending || !bridgeReady}
        />
        <div className="flex justify-end mt-1">
          <button
            onClick={() => void sendMessage()}
            disabled={sending || !bridgeReady || !draft.trim()}
            className="px-3 py-1 text-xs rounded border border-pm-accent bg-pm-accent/20 text-pm-accent disabled:opacity-40"
          >
            {sending ? 'Working…' : 'Send'}
          </button>
        </div>
      </div>
    </div>
  );
}
