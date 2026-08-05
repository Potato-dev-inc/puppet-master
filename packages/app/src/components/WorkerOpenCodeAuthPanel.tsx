import { useCallback, useEffect, useState } from 'react';
import { tauri, type OpenCodeKeyStatus } from '../lib/tauri';

function profileLine(status: OpenCodeKeyStatus | null, id: string): string {
  const profile = status?.profiles.find((entry) => entry.id === id);
  if (!profile) return 'Unknown';
  const active = status?.active_profile === id ? ' · active' : '';
  return profile.configured ? `Configured${active}` : 'Not set';
}

export function WorkerOpenCodeAuthPanel({
  paneId,
  onClose,
}: {
  paneId?: string | null;
  onClose?: () => void;
}) {
  const [keyStatus, setKeyStatus] = useState<OpenCodeKeyStatus | null>(null);
  const [keyA, setKeyA] = useState('');
  const [keyB, setKeyB] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      setKeyStatus(await tauri.getOpenCodeKeyStatus());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveKeys = async () => {
    setBusy(true);
    setError(null);
    try {
      if (keyA.trim()) {
        await tauri.setOpenCodeKeyProfile('a', keyA.trim(), 'Primary');
      }
      if (keyB.trim()) {
        await tauri.setOpenCodeKeyProfile('b', keyB.trim(), 'Secondary');
      }
      setKeyA('');
      setKeyB('');
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const captureFromDisk = async (profileId: 'a' | 'b') => {
    setBusy(true);
    setError(null);
    try {
      await tauri.captureOpenCodeKeyProfile(profileId, profileId === 'a' ? 'Primary' : 'Secondary');
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const rotate = async (profile: 'next' | 'a' | 'b') => {
    setBusy(true);
    setError(null);
    try {
      await tauri.rotateOpenCodeKey(profile, paneId ?? undefined);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="pm-worker-auth-panel" role="dialog" aria-label="OpenCode API key setup">
      <div className="pm-worker-auth-panel-head">
        <div>
          <p className="pm-worker-auth-eyebrow">Worker setup</p>
          <h2>OpenCode API keys</h2>
        </div>
        {onClose && (
          <button type="button" className="pm-worker-auth-close" onClick={onClose} aria-label="Close">
            ×
          </button>
        )}
      </div>

      <p className="pm-worker-auth-hint">
        Stored locally on this machine only. MCP can rotate profiles without seeing key material.
      </p>

      <div className="pm-worker-auth-grid">
        <label>
          Key A — {profileLine(keyStatus, 'a')}
          <input
            type="password"
            value={keyA}
            onChange={(event) => setKeyA(event.target.value)}
            placeholder="Leave blank to keep existing"
            autoComplete="off"
            className="pm-worker-auth-input"
          />
        </label>
        <label>
          Key B — {profileLine(keyStatus, 'b')}
          <input
            type="password"
            value={keyB}
            onChange={(event) => setKeyB(event.target.value)}
            placeholder="Leave blank to keep existing"
            autoComplete="off"
            className="pm-worker-auth-input"
          />
        </label>
      </div>

      <div className="pm-worker-auth-actions">
        <button type="button" onClick={() => void saveKeys()} disabled={busy || (!keyA.trim() && !keyB.trim())}>
          Save keys
        </button>
        <button type="button" onClick={() => void refresh()} disabled={busy}>
          Refresh
        </button>
      </div>

      <div className="pm-worker-auth-actions">
        <button type="button" onClick={() => void captureFromDisk('a')} disabled={busy}>
          Capture A from disk
        </button>
        <button type="button" onClick={() => void captureFromDisk('b')} disabled={busy}>
          Capture B from disk
        </button>
      </div>

      <div className="pm-worker-auth-rotate">
        <span>Active: {keyStatus?.active_profile?.toUpperCase() ?? '—'}</span>
        <div className="pm-worker-auth-actions">
          <button type="button" onClick={() => void rotate('next')} disabled={busy}>
            Toggle
          </button>
          <button type="button" onClick={() => void rotate('a')} disabled={busy}>
            Use A
          </button>
          <button type="button" onClick={() => void rotate('b')} disabled={busy}>
            Use B
          </button>
        </div>
      </div>

      {paneId && (
        <p className="pm-worker-auth-hint">
          Rotate restarts this worker pane ({paneId.slice(0, 8)}…) with the selected key.
        </p>
      )}

      {error && <div className="pm-terminal-error">{error}</div>}
    </div>
  );
}
