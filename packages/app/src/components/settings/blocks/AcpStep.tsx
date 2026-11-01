// The chat (ACP) step shared by the agent wizard and the "Add chat" dialog: what was guessed from the help text (nothing is
// run), or "Enter it myself", and the fields that become the `[acp]` table. Everything the program printed is React text.
import { useEffect, useRef, useState } from 'react';
import { splitError } from '../../../lib/agentWizard';
import {
  candidateLine,
  confidenceNote,
  fieldsFromCandidate,
  validateAcp,
  type AcpDetectResult,
  type AcpFields,
  type AcpValue,
} from '../../../lib/agentWizardAcp';
import { BTN, INPUT } from './AgentWizardCapture';

/** What the chat step keeps while the person goes back and forth. */
export interface AcpDraft {
  fields: AcpFields;
  /** `mine` = "Enter it myself", else the index of the picked candidate. */
  pick: 'mine' | number;
  detected: AcpDetectResult | null;
  detectError: string | null;
  loaded: boolean;
}

/**
 * The chat (ACP) step: what the wizard guessed from the help text (nothing is run), or "Enter it myself", and the fields
 * that become the `[acp]` table. Everything the program printed (the help line) is shown as React text.
 */
export function AcpStep({
  detect,
  detectKey,
  draft,
  onDraft,
  nextLabel,
  onNext,
  onCancel,
  intro,
}: {
  /** Looks for candidates (help text only). `null`: this screen cannot look. */
  detect: (() => Promise<AcpDetectResult>) | null;
  /** The look-up runs once per key. */
  detectKey: string;
  /** The sentence under the title; the default fits the wizard. */
  intro?: string;
  draft: AcpDraft;
  onDraft: (d: AcpDraft) => void;
  nextLabel: string;
  onNext: (value: AcpValue) => void;
  onCancel: () => void;
}) {
  const [tried, setTried] = useState(false);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const check = validateAcp(draft.fields);
  const errors = tried ? check.errors : {};

  useEffect(() => {
    if (draftRef.current.loaded) return;
    let alive = true;
    const finish = (patch: Partial<AcpDraft>) => {
      if (alive) onDraft({ ...draftRef.current, loaded: true, ...patch });
    };
    if (!detect) {
      finish({ detectError: 'This wizard cannot look for the chat command.' });
      return () => {
        alive = false;
      };
    }
    void detect()
      .then((d) => {
        const first = d.candidates[0];
        finish(first ? { detected: d, pick: 0, fields: fieldsFromCandidate(first) } : { detected: d });
      })
      .catch((e) => finish({ detectError: splitError(e).message }));
    return () => {
      alive = false;
    };
    // The look-up runs once per session; `onDraft` only stores state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detectKey]);

  const set = (patch: Partial<AcpFields>) => onDraft({ ...draft, fields: { ...draft.fields, ...patch } });
  const candidates = draft.detected?.candidates ?? [];
  const AREA = 'mt-1 block w-full rounded-lg border border-pm-border bg-transparent px-2 py-1 font-mono text-xs';
  const err = (text?: string) =>
    text ? (
      <span role="alert" className="mt-1 block text-xs text-pm-text">
        {text}
      </span>
    ) : null;

  return (
    <div className="mt-4" data-testid="acp-step">
      <p className="text-sm font-semibold">How does the chat start?</p>
      <p className="mt-1 text-xs text-pm-muted">
        {intro ??
          'Chat agents talk to Puppet Master through a small helper program (the Agent Client Protocol). Pick what looks right, or type it yourself. Nothing is run to find this: the wizard only read the help text.'}
      </p>
      {!draft.loaded ? (
        <p role="status" className="mt-2 text-xs text-pm-muted">
          Reading the help text...
        </p>
      ) : (
        <fieldset className="mt-3">
          <legend className="text-xs font-medium">What the wizard found</legend>
          {draft.detectError && (
            <p data-testid="acp-detect-error" className="mt-1 text-xs text-pm-muted">
              Could not look for it: {draft.detectError} You can still enter it yourself.
            </p>
          )}
          {!draft.detectError && candidates.length === 0 && (
            <p data-testid="acp-no-candidates" className="mt-1 text-xs text-pm-muted">
              Nothing in the help text looked like a chat command. Enter it yourself.
            </p>
          )}
          <div className="mt-2 space-y-2">
            {candidates.map((c, i) => (
              <label key={`${i}-${candidateLine(c)}`} className="flex cursor-pointer items-start gap-2 text-xs">
                <input
                  type="radio"
                  name="wizard-acp-pick"
                  checked={draft.pick === i}
                  onChange={() =>
                    onDraft({ ...draft, pick: i, fields: { ...fieldsFromCandidate(c), envAllow: draft.fields.envAllow, resume: draft.fields.resume } })
                  }
                />
                <span className="min-w-0">
                  <span className="break-all font-mono font-medium">{candidateLine(c)}</span>
                  <span className="block break-words text-pm-muted">From the help: {c.evidence}</span>
                  <span className="block text-pm-muted">{confidenceNote(c)}</span>
                </span>
              </label>
            ))}
            <label className="flex cursor-pointer items-start gap-2 text-xs">
              <input type="radio" name="wizard-acp-pick" checked={draft.pick === 'mine'} onChange={() => onDraft({ ...draft, pick: 'mine' })} />
              <span className="font-medium">Enter it myself</span>
            </label>
          </div>
        </fieldset>
      )}

      <div className="mt-3 space-y-3">
        <label className="block text-xs font-medium" htmlFor="acp-command">
          Command
          <input
            id="acp-command"
            type="text"
            className={`${INPUT} mt-1 block w-full font-mono`}
            value={draft.fields.command}
            onChange={(e) => set({ command: e.target.value })}
          />
          {err(errors.command)}
        </label>
        <label className="block text-xs font-medium" htmlFor="acp-args">
          Arguments
          <span className="block font-normal text-pm-muted">One per line. Type each one as it is, with no shell quotes.</span>
          <textarea id="acp-args" rows={3} className={AREA} value={draft.fields.args} onChange={(e) => set({ args: e.target.value })} />
          {err(errors.args)}
        </label>
        <label className="block text-xs font-medium" htmlFor="acp-env">
          Allowed environment names
          <span className="block font-normal text-pm-muted">
            One per line, for example the name of a key the agent needs. Everything else from the app&apos;s environment is dropped
            before the program starts.
          </span>
          <textarea id="acp-env" rows={3} className={AREA} value={draft.fields.envAllow} onChange={(e) => set({ envAllow: e.target.value })} />
          {err(errors.envAllow)}
        </label>
        <label className="block text-xs font-medium" htmlFor="acp-resume">
          Resume command (optional)
          <span className="block font-normal text-pm-muted">
            One argument per line, and one of them must contain {'{session}'}. It is used only for &quot;Open in terminal&quot;, to
            continue the chat in the agent&apos;s own terminal.
          </span>
          <textarea id="acp-resume" rows={2} className={AREA} value={draft.fields.resume} onChange={(e) => set({ resume: e.target.value })} />
          {err(errors.resume)}
        </label>
      </div>

      <div className="mt-4 flex justify-end gap-2">
        <button type="button" className={BTN} onClick={onCancel}>
          Cancel
        </button>
        <button
          type="button"
          className={BTN}
          onClick={() => {
            setTried(true);
            if (check.ok) onNext(check.value);
          }}
        >
          {nextLabel}
        </button>
      </div>
    </div>
  );
}
