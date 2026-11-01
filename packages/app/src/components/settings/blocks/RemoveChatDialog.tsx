// The question before Chat is removed for an agent: what changes (new sessions start in a terminal, saved chats stay),
// and when an add-on is on disk, a box to delete it too. Plain words, no jargon. Cancel is the default button.
import { useState } from 'react';
import type { AcpLaneAgent } from '../../../lib/acpChat';
import { Btn } from '../settings-ui';

export function RemoveChatDialog({
  agent,
  busy,
  error,
  onRemove,
  onCancel,
}: {
  agent: Pick<AcpLaneAgent, 'label' | 'addon'>;
  busy: boolean;
  error: string | null;
  onRemove: (removeAddon: boolean) => void;
  onCancel: () => void;
}) {
  const [removeAddon, setRemoveAddon] = useState(false);
  const addon = agent.addon?.installed ? agent.addon : null;
  const dialog = (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`Remove chat for ${agent.label}`}
        data-testid="remove-chat-dialog"
        className="w-full max-w-md rounded-xl border border-pm-border bg-pm-panel p-5 text-pm-text"
        onKeyDown={(e) => {
          if (e.key === 'Escape' && !busy) onCancel();
        }}
      >
        <h3 className="text-sm font-semibold">Remove chat for {agent.label}?</h3>
        <p className="mt-2 text-sm text-pm-muted">New sessions will start in a terminal. Your saved chats stay.</p>
        {addon && (
          <label className="mt-3 flex items-center gap-2 text-sm">
            <input type="checkbox" checked={removeAddon} disabled={busy} onChange={(e) => setRemoveAddon(e.target.checked)} />
            <span>Also delete the chat add-on ({addon.size_mb} MB)</span>
          </label>
        )}
        {error && (
          <p role="alert" data-testid="remove-chat-error" className="mt-3 text-sm text-pm-text">
            {error}
          </p>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <Btn small autoFocus disabled={busy} onClick={onCancel}>
            Cancel
          </Btn>
          <Btn small primary disabled={busy} onClick={() => onRemove(!!addon && removeAddon)}>
            {busy ? 'Removing…' : 'Remove'}
          </Btn>
        </div>
      </div>
    </div>
  );
  // Not in a portal: the Settings buttons are styled only inside the Settings window, and a fixed overlay covers it anyway.
  return dialog;
}
