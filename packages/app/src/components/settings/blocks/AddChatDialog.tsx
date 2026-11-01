// "Add chat (ACP)" for an agent that has no chat yet, with nothing to type: it reads the agent's help text (nothing but
// help is run, 20 s at most), takes the best guess and goes straight to one confirm screen with the exact command line.
// "Change details" opens the editable form (the shared `AcpStep`), pre-filled. Found nothing, failed or timed out: it
// says so and shows the form. Only "Add chat" saves anything. Nothing here names an agent: the label comes from the caller.
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { errorSentence, type AcpChatApi, type AcpLaneAgent } from '../../../lib/acpChat';
import { splitError } from '../../../lib/agentWizard';
import { emptyFields, fieldsFromCandidate, validateAcp, type AcpDetectResult, type AcpValue } from '../../../lib/agentWizardAcp';
import { AcpStep, type AcpDraft } from './AcpStep';
import { BTN } from './AgentWizardCapture';

export type AddChatApi = Pick<AcpChatApi, 'overlayDetect' | 'overlaySet'>;

/** How long the dialog waits for the help text before it shows the form. */
export const DETECT_TIMEOUT_MS = 20_000;

/** The sentence on the confirm screen. */
export const confirmSentence = (label: string): string =>
  `Puppet Master will start this program when you open a chat with ${label}. It cannot read your other environment variables except the ones you allow.`;

/** The exact command line that will run. */
export const commandLine = (v: AcpValue): string => [v.command, ...v.args].join(' ');

/** The best guess: the index of the first high-confidence candidate, else 0; -1 when there is none. */
export function bestCandidate(d: AcpDetectResult | null | undefined): number {
  const list = d?.candidates ?? [];
  if (list.length === 0) return -1;
  const high = list.findIndex((c) => c.confidence === 'high');
  return high >= 0 ? high : 0;
}

/** `promise`, or a rejection after `ms`. */
function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Reading the help text took too long.')), ms);
    promise.then(
      (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      (e) => {
        clearTimeout(timer);
        reject(e);
      },
    );
  });
}

const BLANK: AcpDraft = { fields: emptyFields(), pick: 'mine', detected: null, detectError: null, loaded: false };

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
  const [draft, setDraft] = useState<AcpDraft>(BLANK);
  const [value, setValue] = useState<AcpValue | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const started = useRef<string | null>(null);

  useEffect(() => {
    // One look-up per agent, even if React runs the effect twice.
    if (started.current === agent.id) return;
    started.current = agent.id;
    const none = `Could not find how ${agent.label} offers chat. You can type it in.`;
    const show = (patch: Partial<AcpDraft>, message: string | null) => {
      setDraft({ ...BLANK, loaded: true, ...patch });
      setNote(message);
    };
    if (!api.overlayDetect) {
      show({}, none);
      return;
    }
    void withTimeout(
      Promise.resolve().then(() => api.overlayDetect!(agent.id)),
      DETECT_TIMEOUT_MS,
    ).then(
      (found) => {
        const i = bestCandidate(found);
        if (i < 0) return show({ detected: found }, none);
        const fields = fieldsFromCandidate(found.candidates[i]);
        const checked = validateAcp(fields);
        show({ detected: found, pick: i, fields }, checked.ok ? null : none);
        if (checked.ok) setValue(checked.value);
      },
      (e) => show({}, `${none} (${splitError(e).message})`),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agent.id]);

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
        {!draft.loaded ? (
          <p role="status" data-testid="add-chat-reading" className="mt-4 text-xs text-pm-muted">
            Reading the help text...
          </p>
        ) : value === null ? (
          <>
            {note && (
              <p data-testid="add-chat-note" className="mt-3 text-sm">
                {note}
              </p>
            )}
            <AcpStep
              detect={null}
              detectKey={agent.id}
              hideNotes
              intro="Chat talks to the agent through a small helper program (the Agent Client Protocol). Nothing is run to find this except the agent's help."
              draft={draft}
              onDraft={setDraft}
              nextLabel="Review"
              onNext={(v) => {
                setError(null);
                setValue(v);
              }}
              onCancel={onClose}
            />
          </>
        ) : (
          <div className="mt-4" data-testid="add-chat-confirm">
            <p className="text-sm" data-testid="add-chat-summary">
              {agent.label} will start{' '}
              <code data-testid="add-chat-command" className="break-all rounded bg-pm-bg px-1 py-0.5 font-mono text-xs">
                {commandLine(value)}
              </code>{' '}
              for chat.
            </p>
            {value.env_allow.length > 0 && <p className="mt-2 break-all text-xs text-pm-muted">Allowed environment names: {value.env_allow.join(', ')}</p>}
            <p className="mt-3 text-xs text-pm-muted">{confirmSentence(agent.label)}</p>
            <div className="mt-4 flex items-center justify-between gap-2">
              <button
                type="button"
                data-testid="add-chat-change"
                className="text-xs text-pm-muted underline underline-offset-2 hover:text-pm-text"
                disabled={busy}
                onClick={() => setValue(null)}
              >
                Change details
              </button>
              <div className="flex gap-2">
                <button type="button" className={BTN} disabled={busy} onClick={onClose}>
                  Cancel
                </button>
                <button type="button" className={BTN} disabled={busy} onClick={() => void add()}>
                  Add chat
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
  return typeof document === 'undefined' ? dialog : createPortal(dialog, document.body);
}
