// Settings > Agents: "Warn me when two agents change the same file" (on by default). Saves itself the moment you
// flip it, through the collision settings command. It only warns: nothing here pauses or stops an agent.
import { useEffect, useState } from 'react';
import { tauriCollisionApi, type CollisionApi, type CollisionSettings } from '../../../lib/collision';
import { Note, SettingToggle } from '../settings-ui';

export const COLLISION_LABEL = 'Warn me when two agents change the same file';
export const COLLISION_FOOTER =
  'Puppet Master reads the list of changed files, and the changed lines when both agents changed the same file. It never pauses or stops an agent.';
export const COLLISION_SAVE_FAILED = 'Could not save. Try again.';

export function CollisionSettings({ api = tauriCollisionApi, onSaved }: { api?: CollisionApi; onSaved?: () => void }) {
  const [current, setCurrent] = useState<CollisionSettings | null>(null);
  const [status, setStatus] = useState<'loading' | 'ready' | 'unavailable'>('loading');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api.settingsGet().then(
      (value) => {
        if (!alive) return;
        setCurrent(value);
        setStatus('ready');
      },
      () => {
        if (alive) setStatus('unavailable');
      },
    );
    return () => {
      alive = false;
    };
  }, [api]);

  if (status === 'loading') return null;

  const change = (enabled: boolean) => {
    const previous = current;
    if (!previous) return;
    setCurrent({ ...previous, enabled });
    setError(null);
    api.settingsSet({ enabled }).then(
      (saved) => {
        setCurrent(saved);
        onSaved?.();
      },
      () => {
        setCurrent(previous);
        setError(COLLISION_SAVE_FAILED);
      },
    );
  };

  return (
    <>
      <SettingToggle label={COLLISION_LABEL} checked={current?.enabled ?? false} disabled={!current} onChange={change} />
      <Note>{COLLISION_FOOTER}</Note>
      {error && <Note role="alert">{error}</Note>}
    </>
  );
}
