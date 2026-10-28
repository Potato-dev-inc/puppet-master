// "Go live": picks the panes to share, the title, how long and how many viewers, shows exactly what viewers
// will see, then starts the room. After start it shows the link masked; Copy link fetches the full one.
// The owner's own panes are checked by default. Companion and chat-less panes are greyed with the reason.
import { useEffect, useMemo, useState } from 'react';
import {
  durationLabel,
  liveRoomApi,
  maskLink,
  type LiveRoomApi,
  type LiveRoomConfig,
  type PreviewView,
  type StartRequest,
} from '../../../lib/live-room';
import { SegTabs, type SegOption } from '../SegTabs';
import { Sheet } from '../Sheet';
import { copyRoomLink, errorText } from './useLiveRoom';
import { LivePreview } from './LivePreview';
import '../../../styles/cockpit-live.css';

/** A pane the owner could share. `blocked` greys it out with the reason. */
export interface SharePane {
  id: string;
  label: string;
  /** The pane this one is a helper of. Helpers are listed under their parent. */
  parentId?: string | null;
  blocked?: 'companion' | 'no-chat' | null;
}

const BLOCK_TEXT = {
  companion: 'Companion chats stay private',
  'no-chat': 'No chat for this pane',
} as const;

const MINUTE_OPTIONS: SegOption<string>[] = [30, 60, 120, 240, 480].map((m) => ({ id: String(m), label: durationLabel(m) }));
const VIEWER_OPTIONS: SegOption<string>[] = [5, 10, 25, 50].map((n) => ({ id: String(n), label: String(n) }));

const pad = (n: number) => String(n).padStart(2, '0');
/** Local clock time as 24-hour `HH:MM`, for the end time in the sheet. */
export function clockText(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Panes in tree order: each pane, then its helpers (one level deeper). Cycles and orphans are not lost. */
export function orderPanes(panes: SharePane[]): Array<{ pane: SharePane; depth: number }> {
  const ids = new Set(panes.map((p) => p.id));
  const roots = panes.filter((p) => !p.parentId || !ids.has(p.parentId));
  const out: Array<{ pane: SharePane; depth: number }> = [];
  const seen = new Set<string>();
  const walk = (pane: SharePane, depth: number) => {
    if (seen.has(pane.id)) return;
    seen.add(pane.id);
    out.push({ pane, depth });
    for (const child of panes.filter((p) => p.parentId === pane.id)) walk(child, depth + 1);
  };
  roots.forEach((p) => walk(p, 0));
  // Anything a cycle hid still appears, at the top level.
  panes.forEach((p) => {
    if (!seen.has(p.id)) walk(p, 0);
  });
  return out;
}

export function GoLiveSheet({
  panes,
  currentPaneId,
  teamRelay = '',
  api = liveRoomApi,
  onClose,
  onOpenSettings,
  onStarted,
}: {
  panes: SharePane[];
  /** The pane the person is on. Checked by default unless it cannot be shared. */
  currentPaneId?: string;
  /** The team relay address, if a team relay is set. The Live Room relay falls back to it. */
  teamRelay?: string;
  api?: LiveRoomApi;
  onClose: () => void;
  onOpenSettings?: () => void;
  onStarted?: (endsAtMs: number) => void;
}) {
  const initial = panes.find((p) => p.id === currentPaneId && !p.blocked);
  const [picked, setPicked] = useState<string[]>(initial ? [initial.id] : []);
  const [title, setTitle] = useState('Live Room');
  const [config, setConfig] = useState<LiveRoomConfig | null>(null);
  const [minutes, setMinutes] = useState(60);
  const [maxViewers, setMaxViewers] = useState(10);
  const [preview, setPreview] = useState<PreviewView | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [endsAtMs, setEndsAtMs] = useState<number | null>(null);
  const [masked, setMasked] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const pickedKey = picked.join('\n');
  const rows = useMemo(() => orderPanes(panes), [panes]);

  // The defaults the person set in Settings › General › Live Room.
  useEffect(() => {
    let live = true;
    api
      .configGet()
      .then((c) => {
        if (!live) return;
        setConfig(c);
        setMinutes(c.minutes);
        setMaxViewers(c.maxViewers);
      })
      .catch((err) => live && setError(errorText(err)));
    return () => {
      live = false;
    };
  }, [api]);

  // What viewers would see right now: the same items as the first snapshot.
  useEffect(() => {
    const ids = pickedKey ? pickedKey.split('\n') : [];
    if (ids.length === 0) {
      setPreview(null);
      return;
    }
    let live = true;
    api
      .preview(ids)
      .then((v) => live && setPreview(v))
      .catch((err) => live && setError(errorText(err)));
    return () => {
      live = false;
    };
  }, [api, pickedKey]);

  const noRelay = config !== null && !config.relay.trim() && !teamRelay.trim();

  const toggle = (id: string) => setPicked((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));

  /** Checks every helper of a pane that can be shared. Adds; never unchecks. */
  const addHelpers = (parentId: string) => {
    const helpers = panes.filter((p) => p.parentId === parentId && !p.blocked).map((p) => p.id);
    setPicked((cur) => [...cur, ...helpers.filter((id) => !cur.includes(id))]);
  };

  const start = async () => {
    setStarting(true);
    setError(null);
    try {
      const req: StartRequest = { paneIds: picked, title: title.trim() || 'Live Room', minutes, maxViewers };
      const room = await api.start(req);
      // Read once to mask it. The full link is not kept in state; Copy link fetches it again.
      const link = await api.copyLink();
      setMasked(maskLink(link));
      setEndsAtMs(room.endsAtMs);
      onStarted?.(room.endsAtMs);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setStarting(false);
    }
  };

  const copy = () => {
    setError(null);
    copyRoomLink(api)
      .then(() => setCopied(true))
      .catch((err) => setError(errorText(err)));
  };

  if (endsAtMs !== null) {
    return (
      <Sheet labelledBy="ck-live-t" role="dialog" className="ck-sheet--live" onClose={onClose}>
        <h2 className="ck-section" id="ck-live-t">
          You are live
        </h2>
        <p className="ck-live-link" aria-label="Link">
          <code>{masked ?? '••••'}</code>
        </p>
        <div className="ck-live-row">
          <button type="button" className="ck-btn ck-btn--secondary" onClick={copy}>
            {copied ? 'Copied' : 'Copy link'}
          </button>
        </div>
        <p className="ck-sub">{`Anyone with the link can watch until you stop it or it ends at ${clockText(endsAtMs)}.`}</p>
        {error && (
          <p className="ck-live-error" role="alert">
            {error}
          </p>
        )}
        <div className="ck-sheet-actions">
          <button type="button" className="ck-btn ck-btn--primary" autoFocus onClick={onClose}>
            Done
          </button>
        </div>
      </Sheet>
    );
  }

  return (
    <Sheet labelledBy="ck-live-t" role="dialog" className="ck-sheet--live" onClose={starting ? undefined : onClose}>
      <h2 className="ck-section" id="ck-live-t">
        Go live
      </h2>

      <div className="ck-live-field-label">Panes</div>
      <ul className="ck-live-panes">
        {rows.map(({ pane, depth }) => {
          const helpers = panes.filter((p) => p.parentId === pane.id && !p.blocked).length;
          const all = panes.filter((p) => p.parentId === pane.id).length;
          return (
            <li
              key={pane.id}
              className={`ck-live-pane${pane.blocked ? ' is-blocked' : ''}${depth > 0 ? ' is-helper' : ''}`}
              style={depth > 0 ? { paddingLeft: depth * 18 } : undefined}
            >
              <label className="ck-live-pane-check">
                <input
                  type="checkbox"
                  checked={!pane.blocked && picked.includes(pane.id)}
                  disabled={Boolean(pane.blocked) || starting}
                  onChange={() => toggle(pane.id)}
                />
                <span className="ck-live-pane-name">{pane.label}</span>
                {pane.blocked && <span className="ck-live-pane-why">{BLOCK_TEXT[pane.blocked]}</span>}
              </label>
              {depth === 0 && all > 0 && !pane.blocked && (
                <button type="button" className="ck-live-helpers" disabled={helpers === 0 || starting} onClick={() => addHelpers(pane.id)}>
                  {`and its ${all} ${all === 1 ? 'helper' : 'helpers'}`}
                </button>
              )}
            </li>
          );
        })}
      </ul>

      <label className="ck-live-field">
        <span className="ck-live-field-label">Title</span>
        <input className="ck-live-input" value={title} maxLength={60} disabled={starting} onChange={(e) => setTitle(e.target.value)} />
      </label>

      <div className="ck-live-field">
        <span className="ck-live-field-label">Ends after</span>
        <SegTabs options={MINUTE_OPTIONS} value={String(minutes)} onChange={(v) => setMinutes(Number(v))} label="Ends after" small />
      </div>

      <div className="ck-live-field">
        <span className="ck-live-field-label">Viewers</span>
        <SegTabs options={VIEWER_OPTIONS} value={String(maxViewers)} onChange={(v) => setMaxViewers(Number(v))} label="Viewers" small />
      </div>

      <div className="ck-live-field-label">What viewers see</div>
      <LivePreview items={preview?.items ?? []} hidden={preview?.hidden ?? 0} panes={preview?.panes} />

      <p className="ck-sub ck-live-help">
        Viewers see the chat and what each agent is doing. Secrets, names and folders are hidden. They never see the terminal or your
        questions, and they cannot type.
      </p>

      {noRelay && (
        <div className="ck-live-norelay">
          <p className="ck-sub" role="status">
            Live Room needs a relay address. Set it in Settings › General › Live Room.
          </p>
          {onOpenSettings && (
            <button type="button" className="ck-btn ck-btn--secondary" onClick={onOpenSettings}>
              Open Live Room settings
            </button>
          )}
        </div>
      )}

      {error && (
        <p className="ck-live-error" role="alert">
          {error}
        </p>
      )}

      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--plain" disabled={starting} onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="ck-btn ck-btn--primary"
          disabled={picked.length === 0 || !config || noRelay || starting}
          onClick={() => void start()}
        >
          Go live
        </button>
      </div>
    </Sheet>
  );
}
