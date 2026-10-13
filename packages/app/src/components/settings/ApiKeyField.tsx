// One provider API key: masked when stored, with Replace / Remove. The copy matches where the key really is.
import { useCallback, useEffect, useState } from 'react';
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
import { FieldInput } from './settings-ui';

interface Props {
  label: string;
  name: SecretName;
  placeholder: string;
  api?: SecretsApi;
  /** Value held in settings.json. Only used when there is no secure storage. */
  fileValue?: string;
  onFileChange?: (value: string | undefined) => void;
}

export function ApiKeyField({ label, name, placeholder, api = secretsApi, fileValue, onFileChange }: Props) {
  const [info, setInfo] = useState<SecretStorageInfo | null>(null);
  const [mask, setMask] = useState<SecretMask | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const i = await api.storageInfo();
      setInfo(i);
      setMask(i.secure ? await api.getMasked(name) : null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
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

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(async () => {
      const value = draft.trim();
      if (!value) return;
      if (info.secure) {
        await api.set(name, value);
        await refresh();
      } else {
        onFileChange?.(value);
      }
      setDraft('');
      setEditing(false);
    });

  const remove = () =>
    run(async () => {
      if (info.secure) {
        await api.remove(name);
        await refresh();
      } else {
        onFileChange?.(undefined);
      }
      setEditing(false);
    });

  const btn = 'rounded-lg border border-pm-border px-3 py-1.5 text-sm hover:bg-pm-border/40 disabled:opacity-50';

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
              onChange={(e) => setDraft(e.target.value)}
              className="font-mono"
              placeholder={placeholder}
              autoComplete="off"
            />
            <button type="button" className={btn} disabled={busy || !draft.trim()} onClick={() => void save()}>
              Save
            </button>
            {editing && (
              <button
                type="button"
                className={btn}
                onClick={() => {
                  setEditing(false);
                  setDraft('');
                }}
              >
                Cancel
              </button>
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
          <button type="button" className={btn} disabled={busy} onClick={() => setEditing(true)}>
            Replace
          </button>
          <button type="button" className={btn} disabled={busy} onClick={() => void remove()}>
            Remove
          </button>
        </div>
      )}
      {error && (
        <p role="alert" className="text-xs text-red-300">
          {error}
        </p>
      )}
    </div>
  );
}
