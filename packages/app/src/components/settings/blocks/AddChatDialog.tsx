// "Add chat (ACP)" for an agent that has no chat yet: find how it starts as a chat from its help text (nothing but help
// is run), let the person pick or type it (the shared `AcpStep`), then show the exact command line and ask once more.
// Only "Add chat" saves anything. Nothing here names an agent: the label comes from the caller.
import { useState } from 'react';
import { createPortal } from 'react-dom';
import { errorSentence, type AcpChatApi, type AcpLaneAgent } from '../../../lib/acpChat';
import { emptyFields, type AcpValue } from '../../../lib/agentWizardAcp';
import { AcpStep, type AcpDraft } from './AcpStep';
import { BTN } from './AgentWizardCapture';

export type AddChatApi = Pick<AcpChatApi, 'overlayDetect' | 'overlaySet'>;

/** The sentence on the confirm screen. */
export const confirmSentence = (label: string): string =>
  `Puppet Master will start this program when you open a chat with ${label}. It cannot read your other environment variables except the ones you allow.`;

/** The exact command line that will run. */
export const commandLine = (v: AcpValue): string => [v.command, ...v.args].join(' ');

export function AddChatDialog({
  agent,
  api,
  onClose,
  onAdded,
}: {
  agent: { id: string; label: string };
  api: AddChatApi;
  onClose: () => void;
  /** The chat was added; `row` is the agent's lane row from the backend. */
  onAdded: (row: AcpLaneAgent | null) => void;
}) {
  const [draft, setDraft] = useState<AcpDraft>({ fields: emptyFields(), pick: 'mine', detected: null, detectError: null, loaded: false });
  const [value, setValue] = useState<AcpValue | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const add = async () => {
    if (!value || busy) return;
    if (!api.overlaySet) {
      setError('This version of the app cannot add a chat.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const row = await api.overlaySet(agent.id, value, true);
      onAdded(row ?? null);
    } catch (e) {
      setError(errorSentence(e));
      setBusy(false);
    }
  };

  const dialog = (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`Add chat for ${agent.label}`}
        data-testid="add-chat-dialog"
        className="max-h-full w-full max-w-xl overflow-auto rounded-xl border border-pm-border bg-pm-panel p-5 text-pm-text"
      >
        <h3 className="text-sm font-semibold">Add chat for {agent.label}</h3>
        {error && (
          <p role="alert" data-testid="add-chat-error" className="mt-3 text-sm text-pm-text">
            {error}
          </p>
        )}
        {value === null ? (
          <AcpStep
            detect={api.overlayDetect ? () => Promise.resolve(api.overlayDetect!(agent.id)) : null}
            detectKey={agent.id}
            intro="Chat talks to the agent through a small helper program (the Agent Client Protocol). Pick what looks right, or type it yourself. Nothing is run to find this except the agent's help."
            draft={draft}
            onDraft={setDraft}
            nextLabel="Review"
            onNext={(v) => {
              setError(null);
              setValue(v);
            }}
            onCancel={onClose}
          />
        ) : (
          <div className="mt-4" data-testid="add-chat-confirm">
            <p className="text-xs text-pm-muted">This is the exact command line:</p>
            <pre data-testid="add-chat-command" className="mt-1 overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-pm-bg p-2 text-xs">
              {commandLine(value)}
            </pre>
            {value.env_allow.length > 0 && <p className="mt-2 break-all text-xs text-pm-muted">Allowed environment names: {value.env_allow.join(', ')}</p>}
            <p className="mt-3 text-sm">{confirmSentence(agent.label)}</p>
            <div className="mt-4 flex justify-end gap-2">
              <button type="button" className={BTN} disabled={busy} onClick={onClose}>
                Cancel
              </button>
              <button type="button" className={BTN} disabled={busy} onClick={() => setValue(null)}>
                Back
              </button>
              <button type="button" className={BTN} disabled={busy} onClick={() => void add()}>
                Add chat
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
  return typeof document === 'undefined' ? dialog : createPortal(dialog, document.body);
}
