// One provider API key: masked when stored, with Replace / Remove. The copy matches where the key really is.
// Typing a key saves it when the field loses focus or on Enter (never per keystroke, no Save button).
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  NO_SECURE_STORAGE_WARNING,
  describeKeyState,
  maskLocal,
  secretsApi,
  type SecretMask,
  type SecretName,
  type SecretStorageInfo,
  type SecretsApi,
} from '../../lib/secrets';
import { Btn, FieldInput, Note } from './settings-ui';

interface Props {
  label: string;
  name: SecretName;
  placeholder: string;
  api?: SecretsApi;
  /** Value held in settings.json. Only used when there is no secure storage. */
  fileValue?: string;
  /** Fallback mode only: write the key (or `undefined` to remove it) into settings.json. */
  onFileChange?: (value: string | undefined) => void | Promise<void>;
  /** Called after a key is saved or removed in secure storage (not for the settings-file fallback). */
  onSaved?: () => void;
  /** Asked before a stored key is removed. Resolves false to keep it. */
  onConfirmRemove?: () => Promise<boolean>;
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function ApiKeyField({ label, name, placeholder, api = secretsApi, fileValue, onFileChange, onSaved, onConfirmRemove }: Props) {
  const [info, setInfo] = useState<SecretStorageInfo | null>(null);
  const [mask, setMask] = useState<SecretMask | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  // Mirrors `draft` so a blur right after Enter (or a second blur) cannot save the same key twice.
  const draftRef = useRef('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const i = await api.storageInfo();
      setInfo(i);
      setMask(i.secure ? await api.getMasked(name) : null);
    } catch (e) {
      setError(errorText(e));
    }
  }, [api, name]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (!info) {
    return (
      <p className="text-xs text-pm-muted">
        {error ? `Could not check secure storage: ${error}` : 'Checking where keys are stored...'}
      </p>
    );
  }

  const current = info.secure ? (mask ?? { set: false, last4: null }) : maskLocal(fileValue);
  const showInput = editing || !current.set;

  const setDraftText = (text: string) => {
    draftRef.current = text;
    setDraft(text);
  };

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  // Same logic as the old Save button: trim, skip when empty, store, then clear the field.
  // The draft is cleared before the write, so a second blur or Enter finds nothing to save; it is put
  // back if the write fails so the person can try again.
  const commitDraft = () => {
    const value = draftRef.current.trim();
    if (!value) return;
    setDraftText('');
    void run(async () => {
      try {
        if (info.secure) {
          await api.set(name, value);
          await refresh();
          onSaved?.();
        } else {
          await onFileChange?.(value);
        }
        setEditing(false);
      } catch (e) {
        setDraftText(value);
        throw e;
      }
    });
  };

  const remove = async () => {
    if (onConfirmRemove && !(await onConfirmRemove())) return;
    await run(async () => {
      if (info.secure) {
        await api.remove(name);
        await refresh();
        onSaved?.();
      } else {
        await onFileChange?.(undefined);
      }
      setEditing(false);
    });
  };

  return (
    <div className="space-y-2" data-testid={`key-${name}`}>
      {!info.secure && (
        <p role="note" className="text-xs leading-5 text-pm-muted">
          {NO_SECURE_STORAGE_WARNING}
        </p>
      )}
      {showInput ? (
        <>
          <div className="flex items-center gap-2">
            <FieldInput
              type="password"
              aria-label={label}
              value={draft}
              onChange={(e) => setDraftText(e.target.value)}
              onBlur={commitDraft}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitDraft();
              }}
              className="font-mono"
              placeholder={placeholder}
              autoComplete="off"
            />
            {editing && (
              <Btn
                // Keep focus in the field, so clicking Cancel does not first blur-save what was typed.
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  setEditing(false);
                  setDraftText('');
                }}
              >
                Cancel
              </Btn>
            )}
          </div>
          {!current.set && (
            <p className="text-xs text-pm-muted" data-testid="key-state">
              {describeKeyState(current, info)}
            </p>
          )}
        </>
      ) : (
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm" data-testid="key-state">
            {describeKeyState(current, info)}
          </span>
          <Btn disabled={busy} onClick={() => setEditing(true)}>
            Replace
          </Btn>
          <Btn disabled={busy} onClick={() => void remove()}>
            Remove
          </Btn>
        </div>
      )}
      {error && <Note role="alert">{error}</Note>}
    </div>
  );
}
