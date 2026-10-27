// Messages the person wrote that the agent has not shown in the conversation yet: waiting, being sent, just sent,
// or failed. They sit at the bottom of the chat as bubbles; the real message replaces the bubble once the agent's
// session (or the screen) shows it.
import { useState } from 'react';
import type { OutboxMessage } from '../../lib/paneOutbox';

export interface PendingMessagesProps {
  messages: readonly OutboxMessage[];
  /** Cancels a waiting message. Resolves to null, or to the reason it could not be cancelled. */
  onCancel: (messageId: string) => Promise<string | null>;
  /** Takes a failed message off the list. */
  onHide: (messageId: string) => void;
}

async function copy(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

function Bubble({ message, onCancel, onHide }: { message: OutboxMessage; onCancel: PendingMessagesProps['onCancel']; onHide: (id: string) => void }) {
  const [note, setNote] = useState<string | null>(null);
  const { state } = message;
  return (
    <div className="flex justify-end ck-chat-pending" data-testid="chat-pending" data-state={state}>
      <div className={`max-w-[85%] ck-chat-pending-body${state === 'failed' ? ' ck-chat-pending-body--failed' : ''}`}>
        <div className="whitespace-pre-wrap break-words ck-chat-bubble ck-chat-bubble--pending">{message.text}</div>
        <div className="ck-chat-pending-meta">
          {state === 'queued' && (
            <>
              <span data-testid="chat-pending-waiting">{message.waitingFor ?? 'Waiting…'}</span>
              <button
                type="button"
                className="ck-chat-link"
                onClick={() => {
                  void onCancel(message.id).then(setNote);
                }}
              >
                Cancel
              </button>
            </>
          )}
          {state === 'sending' && <span>Sending…</span>}
          {state === 'sent' && <span>Sent</span>}
          {state === 'failed' && (
            <>
              <span role="alert" data-testid="chat-pending-error">
                {message.error ?? 'Could not send the message.'}
              </span>
              <button
                type="button"
                className="ck-chat-link"
                onClick={() => {
                  void copy(message.text).then((ok) => setNote(ok ? 'Copied.' : 'Could not copy.'));
                }}
              >
                Copy text
              </button>
              <button type="button" className="ck-chat-link" onClick={() => onHide(message.id)}>
                Hide
              </button>
            </>
          )}
          {note && <span data-testid="chat-pending-note">{note}</span>}
        </div>
      </div>
    </div>
  );
}

export function PendingMessages({ messages, onCancel, onHide }: PendingMessagesProps) {
  if (messages.length === 0) return null;
  return (
    <div className="space-y-2 ck-chat-pending-list" aria-live="polite">
      {messages.map((message) => (
        <Bubble key={message.id} message={message} onCancel={onCancel} onHide={onHide} />
      ))}
    </div>
  );
}
