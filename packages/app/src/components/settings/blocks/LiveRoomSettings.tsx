// Settings › General › Live Room: the relay address, the default length and viewers for a new room.
// The relay is checked by the app when saved (https for a remote host); its refusal is shown as is.
import { useEffect, useState } from 'react';
import { durationLabel, liveRoomApi, type LiveRoomApi, type LiveRoomConfig } from '../../../lib/live-room';
import { FieldInput, FieldSelect, Note, SettingRow } from '../settings-ui';

const MINUTES = [30, 60, 120, 240, 480];
const VIEWERS = [5, 10, 25, 50];

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

export function LiveRoomSettings({
  api = liveRoomApi,
  teamRelay = '',
  onSaved,
}: {
  api?: LiveRoomApi;
  /** The team relay address when a team relay is set. Shown as the placeholder of the relay field. */
  teamRelay?: string;
  onSaved?: () => void;
}) {
  const [cfg, setCfg] = useState<LiveRoomConfig | null>(null);
  const [relay, setRelay] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .configGet()
      .then((c) => {
        if (!live) return;
        setCfg(c);
        setRelay(c.relay);
      })
      .catch((err) => live && setError(errText(err)));
    return () => {
      live = false;
    };
  }, [api]);

  if (!cfg) return error ? <Note role="alert">{error}</Note> : null;

  /** Saves the whole config. On a refusal the field keeps what the person typed and the reason shows. */
  const save = (next: LiveRoomConfig) => {
    setError(null);
    api
      .configSet(next)
      .then(() => {
        setCfg(next);
        onSaved?.();
      })
      .catch((err) => setError(errText(err)));
  };

  const saveRelay = () => {
    const next = relay.trim();
    if (next === cfg.relay) return;
    save({ ...cfg, relay: next });
  };

  return (
    <>
      <SettingRow label="Relay address" description="The server that carries the room. Use your own, or the team relay.">
        <FieldInput
          aria-label="Relay address"
          value={relay}
          placeholder={teamRelay.trim() ? 'Uses your team relay' : 'https://relay.example.com'}
          onChange={(e) => setRelay(e.target.value)}
          onBlur={saveRelay}
          onKeyDown={(e) => {
            if (e.key === 'Enter') saveRelay();
          }}
          spellCheck={false}
        />
      </SettingRow>
      <SettingRow label="Default length" description="How long a new room runs.">
        <FieldSelect
          aria-label="Default length"
          value={String(cfg.minutes)}
          onChange={(e) => save({ ...cfg, relay: relay.trim(), minutes: Number(e.target.value) })}
        >
          {MINUTES.map((m) => (
            <option key={m} value={m}>
              {durationLabel(m)}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>
      <SettingRow label="Default viewers" description="Most people who can watch at once.">
        <FieldSelect
          aria-label="Default viewers"
          value={String(cfg.maxViewers)}
          onChange={(e) => save({ ...cfg, relay: relay.trim(), maxViewers: Number(e.target.value) })}
        >
          {VIEWERS.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>
      {error && <Note role="alert">{error}</Note>}
      <Note>Rooms are end-to-end encrypted. The relay sees only sizes and times.</Note>
    </>
  );
}
