// "Add chat (ACP)" for an agent that has no chat yet, with nothing to type and ONE confirm click. It asks the app how the
// agent can get a chat (`overlayResolve`), best first: (1) the program already on this PC starts chat itself, (2) the
// official ACP registry lists a ready-made adapter (downloaded and checked by the app after the click), (3) the built-in
// bridge (Puppet Master makes the chat from the agent's non-interactive mode; the last resort, selectable only when the
// agent has one, with no download). The first usable way is pre-selected and shown on one confirm screen; with more
// than one usable way they are radio choices. "Change details" opens the editable form (the shared `AcpStep`), pre-filled.
// Nothing found, failed or timed out: it says so and shows the form. Only "Add chat" saves or downloads anything.
// An older app without `overlayResolve` falls back to reading the help text alone. Nothing here names an agent: the label
// comes from the caller, and everything the registry says is shown as plain text.
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import {
  errorSentence,
  bridgeSentence,
  firstAvailable,
  normalizeOptions,
  registrySentence,
  type AcpChatApi,
  type AcpLaneAgent,
  type OverlayOption,
  type OverlayProgress,
} from '../../../lib/acpChat';
import { splitError } from '../../../lib/agentWizard';
import { emptyFields, fieldsFromCandidate, validateAcp, type AcpDetectResult, type AcpValue } from '../../../lib/agentWizardAcp';
import { AcpStep, type AcpDraft } from './AcpStep';
import { BTN } from './AgentWizardCapture';

export type AddChatApi = Pick<AcpChatApi, 'overlayDetect' | 'overlaySet'> & Partial<Pick<AcpChatApi, 'overlayResolve' | 'overlayInstall' | 'overlayProgress'>>;

/** How long the dialog waits for the help text before it shows the form. */
export const DETECT_TIMEOUT_MS = 20_000;
/** How long it waits for the whole look-up (help text and registry). */
export const RESOLVE_TIMEOUT_MS = 45_000;

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
function withTimeout<T>(promise: Promise<T>, ms: number, message = 'Reading the help text took too long.'): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), ms);
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

/** The newest progress lines kept on screen. */
const SHOWN_LINES = 6;

export function AddChatDialog({
  agent,
  api,
  onClose,
  onAdded,
}: {
  agent: { id: string; label: string };
  api: AddChatApi;
  onClose: () => void;
  /** The chat was added; `row` is the agent's lane row from the backend (null when the backend did not send one). */
  onAdded: (row: AcpLaneAgent | null) => void;
}) {
  const [draft, setDraft] = useState<AcpDraft>(BLANK);
  const [value, setValue] = useState<AcpValue | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The ways the app offered; `null` = this app cannot say (the help text alone is used). */
  const [options, setOptions] = useState<OverlayOption[] | null>(null);
  const [sel, setSel] = useState(-1);
  /** `true`: the person is typing or reviewing their own command (the form), not one of the offered ways. */
  const [manual, setManual] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [done, setDone] = useState<string | null>(null);
  const started = useRef<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    // One look-up per agent, even if React runs the effect twice.
    if (started.current === agent.id) return;
    started.current = agent.id;
    const none = `Could not find how ${agent.label} offers chat. You can type it in.`;
    const show = (patch: Partial<AcpDraft>, message: string | null) => {
      setDraft({ ...BLANK, loaded: true, ...patch });
      setNote(message);
    };
    // The older, help-text-only look-up.
    const detectOnly = () => {
      setManual(true);
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
    };
    if (!api.overlayResolve) {
      detectOnly();
      return;
    }
    void withTimeout(
      Promise.resolve().then(() => api.overlayResolve!(agent.id)),
      RESOLVE_TIMEOUT_MS,
      'Looking for a way to chat took too long.',
    ).then(
      (found) => {
        const list = normalizeOptions(found);
        const first = firstAvailable(list);
        setOptions(list);
        if (first >= 0) {
          setSel(first);
          show({}, null);
          return;
        }
        // Nothing can be used: say why (the first reason the app gave), then the form.
        const why = list.find((o) => o.kind !== 'bridge' && o.reason)?.reason;
        setManual(true);
        show({}, why ? `${none} (${why})` : none);
      },
      // An app that has the command but fails, or an older one: the help text alone still works.
      () => detectOnly(),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agent.id]);

  const available = (options ?? []).map((o, i) => ({ o, i })).filter(({ o }) => o.available);
  const option = !manual && options && sel >= 0 ? options[sel] : undefined;
  // What the confirm screen would save for a program that is already installed.
  const shownValue: AcpValue | null = manual ? value : option?.kind === 'native' && option.acp ? { ...option.acp, env_allow: option.acp.env_allow ?? [] } : null;

  const add = async () => {
    if (busy) return;
    if (option?.kind === 'registry') return void install(option);
    if (option?.kind === 'bridge') return void addBridge();
    if (!shownValue) return;
    if (!api.overlaySet) {
      setError('This version of the app cannot add a chat.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const row = await api.overlaySet(agent.id, shownValue, true);
      onAdded(row ?? null);
    } catch (e) {
      setError(errorSentence(e));
      setBusy(false);
    }
  };

  const addBridge = async () => {
    if (!api.overlayInstall) {
      setError('This version of the app cannot add a chat.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const row = await api.overlayInstall(agent.id, { kind: 'bridge' }, true);
      onAdded((row as AcpLaneAgent | null | undefined) ?? null);
    } catch (e) {
      setError(errorSentence(e));
      setBusy(false);
    }
  };

  const install = async (picked: OverlayOption) => {
    if (!api.overlayInstall) {
      setError('This version of the app cannot download a chat add-on.');
      return;
    }
    const info = picked.registry;
    setBusy(true);
    setInstalling(true);
    setError(null);
    setLines([]);
    let stop: (() => void) | undefined;
    try {
      stop = api.overlayProgress ? await api.overlayProgress(agent.id, (p: OverlayProgress) => alive.current && p.lines.length > 0 && setLines(p.lines.slice(-SHOWN_LINES))) : undefined;
    } catch {
      stop = undefined; // progress is a nicety
    }
    try {
      await api.overlayInstall(agent.id, { kind: 'registry', id: info?.id, version: info?.version }, true);
      if (alive.current) setDone(info ? `${info.name} ${info.version}` : picked.label);
    } catch (e) {
      if (alive.current) setError(errorSentence(e));
    } finally {
      stop?.();
      if (alive.current) {
        setInstalling(false);
        setBusy(false);
      }
    }
  };

  const toForm = () => {
    // "Change details": the form, pre-filled with the program of the selected way when it has one.
    if (manual) {
      setValue(null); // already typing: back to the form with what was typed
      return;
    }
    if (option?.kind === 'native' && option.acp) {
      const fields = fieldsFromCandidate({ command: option.acp.command, args: option.acp.args, evidence: '', confidence: 'high' });
      setDraft({ ...BLANK, loaded: true, fields });
    } else {
      setDraft({ ...BLANK, loaded: true });
    }
    setValue(null);
    setManual(true);
  };

  const radios = !manual && available.length > 1 && (
    <div role="radiogroup" aria-label="How to add chat" data-testid="add-chat-options" className="mt-3 flex flex-col gap-2">
      {available.map(({ o, i }) => (
        <label key={`${o.kind}-${i}`} className="flex cursor-pointer items-start gap-2 text-sm" data-testid={`add-chat-option-${o.kind}`}>
          <input type="radio" name="add-chat-option" checked={sel === i} disabled={busy} onChange={() => setSel(i)} className="mt-1" />
          <span>
            <span className="block">{o.kind === 'native' ? 'Use the program already on this computer' : o.kind === 'registry' ? 'Download a ready-made one from the official ACP registry' : 'Let Puppet Master make one (text only)'}</span>
            <span className="block text-xs text-pm-muted">{o.kind === 'native' ? (o.command_line ?? o.summary) : o.label}</span>
          </span>
        </label>
      ))}
    </div>
  );

  const confirm = (
    <div className="mt-4" data-testid="add-chat-confirm">
      {radios}
      {option?.kind === 'registry' && option.registry ? (
        <div className={radios ? 'mt-3' : ''}>
          <p className="text-sm" data-testid="add-chat-summary">
            {registrySentence(option.registry)}
          </p>
          {option.summary && (
            <p className="mt-2 break-words text-xs text-pm-muted" data-testid="add-chat-registry-about">
              {option.summary}
            </p>
          )}
          {option.registry.repository && <p className="mt-1 break-all text-xs text-pm-muted">Source: {option.registry.repository}</p>}
          <p className="mt-3 text-xs text-pm-muted">
            It is installed in the app&apos;s own folder, checked before every start, and cannot read your other environment variables except the ones you allow.
          </p>
        </div>
      ) : option?.kind === 'bridge' ? (
        <div className={radios ? 'mt-3' : ''}>
          <p className="text-sm" data-testid="add-chat-summary">
            {bridgeSentence(agent.label, option.bridge?.has_memory === true)}
          </p>
        </div>
      ) : shownValue ? (
        <div className={radios ? 'mt-3' : ''}>
          <p className="text-sm" data-testid="add-chat-summary">
            {agent.label} will start{' '}
            <code data-testid="add-chat-command" className="break-all rounded bg-pm-bg px-1 py-0.5 font-mono text-xs">
              {commandLine(shownValue)}
            </code>{' '}
            for chat.
          </p>
          {shownValue.env_allow.length > 0 && <p className="mt-2 break-all text-xs text-pm-muted">Allowed environment names: {shownValue.env_allow.join(', ')}</p>}
          <p className="mt-3 text-xs text-pm-muted">{confirmSentence(agent.label)}</p>
        </div>
      ) : null}
      <div className="mt-4 flex items-center justify-between gap-2">
        <button
          type="button"
          data-testid="add-chat-change"
          className="text-xs text-pm-muted underline underline-offset-2 hover:text-pm-text"
          disabled={busy}
          onClick={toForm}
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
  );

  const body = () => {
    if (!draft.loaded) {
      return (
        <p role="status" data-testid="add-chat-reading" className="mt-4 text-xs text-pm-muted">
          {api.overlayResolve ? 'Looking for the best way to add chat...' : 'Reading the help text...'}
        </p>
      );
    }
    if (done) {
      return (
        <div className="mt-4" data-testid="add-chat-done">
          <p role="status" className="text-sm" data-testid="add-chat-success">
            {done} is ready. Chat is added for {agent.label}.
          </p>
          <div className="mt-4 flex justify-end">
            <button type="button" className={BTN} onClick={() => onAdded(null)}>
              Done
            </button>
          </div>
        </div>
      );
    }
    if (installing) {
      return (
        <div className="mt-4" data-testid="add-chat-installing">
          <p role="status" className="text-sm">
            Downloading and setting up chat for {agent.label}. This can take a minute.
          </p>
          {lines.length > 0 && (
            <ul data-testid="add-chat-progress" className="mt-2 max-h-32 overflow-hidden text-xs text-pm-muted">
              {lines.map((l, i) => (
                <li key={`${i}-${l}`} className="break-all">
                  {l}
                </li>
              ))}
            </ul>
          )}
        </div>
      );
    }
    if (manual && value === null) {
      return (
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
      );
    }
    return confirm;
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
        {body()}
      </div>
    </div>
  );
  return typeof document === 'undefined' ? dialog : createPortal(dialog, document.body);
}
