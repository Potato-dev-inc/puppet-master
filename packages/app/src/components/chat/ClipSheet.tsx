// "Make a clip": a short video of one finished turn. The sheet shows it being drawn, then the video exactly as the
// file will be, with the request as an editable caption. Save asks for a place and writes that same file. Rust
// already hid secrets and folder names in the plan; the person looks at the preview before sharing. Nothing is
// uploaded. The file is made here in the webview and saved through the system dialog.
import { useId, type KeyboardEvent } from 'react';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { clipPlan, clipSave, clipStage, type ClipExt } from '../../lib/clip';
import { renderClip } from '../../lib/clipEncode';
import { REQUEST_MAX_CHARS } from '../../lib/clipTimeline';
import '../../styles/cockpit-clip.css';
import { Sheet } from '../cockpit/Sheet';
import { useClipMaker, type ClipApi, type MadeClip } from './useClipMaker';

export type { ClipApi } from './useClipMaker';

const FILTERS: Record<ClipExt, { name: string; extensions: string[] }> = {
  mp4: { name: 'MP4 video', extensions: ['mp4'] },
  webm: { name: 'WebM video', extensions: ['webm'] },
};

export const tauriClipApi: ClipApi = {
  plan: clipPlan,
  render: renderClip,
  stage: clipStage,
  save: clipSave,
  pickPath: async (defaultName, ext) => {
    const picked = await saveDialog({ defaultPath: defaultName, filters: [FILTERS[ext]] });
    return typeof picked === 'string' ? picked : null;
  },
};

export const FALLBACK_NOTE = 'Your system cannot make video here. Using the slower way.';

function plural(n: number, one: string, many: string): string {
  return n === 1 ? one : many;
}

function megabytes(bytes: number): string {
  return `${Math.max(0.1, Math.round(bytes / 105_000) / 10)} MB`;
}

/** The length as the video has it, to a tenth of a second: 28.5 s stays 28.5 s, 23 s stays 23 s. */
export function clipSeconds(totalMs: number): string {
  const tenths = Math.round(Math.max(0, totalMs) / 100);
  const whole = Math.floor(tenths / 10);
  return tenths % 10 === 0 ? `${whole} s` : `${whole}.${tenths % 10} s`;
}

/** The length and size line under the video. */
export function clipFacts(made: MadeClip): string {
  return `${clipSeconds(made.timeline.totalMs)} · ${made.ext.toUpperCase()} · ${megabytes(made.blob.size)}`;
}

export function ClipSheet({
  paneId,
  userItemId,
  api = tauriClipApi,
  onClose,
}: {
  paneId: string;
  /** The message that starts the turn. Without it the last turn. */
  userItemId?: string;
  api?: ClipApi;
  onClose: () => void;
}) {
  const titleId = useId();
  const clip = useClipMaker(paneId, userItemId, api);
  const { state, plan, caption } = clip;
  const made = state.phase === 'ready' || state.phase === 'saving' || state.phase === 'saved' ? state.made : null;
  const working = state.phase === 'planning' || state.phase === 'drawing';
  const saving = state.phase === 'saving';
  const canSave = state.phase === 'ready' || state.phase === 'saved';
  const canEdit = plan !== null && state.phase !== 'planning' && state.phase !== 'error' && !saving;
  const changed = made !== null && made.caption !== caption;
  const percent = state.phase === 'drawing' ? Math.round(state.progress * 100) : 0;

  const onCaptionKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    clip.redraw();
  };

  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={saving ? undefined : onClose} className="ck-clip">
      <h2 id={titleId} className="ck-section">
        Make a clip
      </h2>
      <p className="ck-sub">A short video of this turn. It stays on your PC until you share it. Nothing is uploaded.</p>

      <label className="ck-clip-field">
        <span className="ck-clip-label">What you asked</span>
        <input
          type="text"
          className="ck-clip-input"
          value={caption}
          maxLength={REQUEST_MAX_CHARS}
          disabled={!canEdit}
          onChange={(event) => clip.setCaption(event.target.value)}
          onKeyDown={onCaptionKey}
        />
      </label>
      {changed && (
        <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm ck-clip-redraw" onClick={clip.redraw}>
          Draw again
        </button>
      )}

      <div className="ck-clip-stage">
        {state.phase === 'planning' && <p className="ck-sub">Reading the turn…</p>}
        {state.phase === 'drawing' && (
          <div className="ck-clip-progress">
            <div className="ck-clip-bar" role="progressbar" aria-label="Drawing the clip" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
              <span className="ck-clip-bar-fill" style={{ width: `${percent}%` }} />
            </div>
            <p className="ck-sub">Drawing {percent}%</p>
          </div>
        )}
        {made !== null && <video className="ck-clip-video" src={made.url} controls muted loop autoPlay playsInline aria-label="Clip preview" />}
        {state.phase === 'error' && (
          <p role="alert" className="ck-clip-error">
            {state.message}
          </p>
        )}
      </div>

      {made !== null && (
        <div className="ck-clip-info">
          <p className="ck-sub">{clipFacts(made)}</p>
          <p className="ck-sub">Look at it before you share it. It shows what the agent printed on screen.</p>
          {made.usedFallback && (
            <p role="alert" className="ck-clip-note">
              {FALLBACK_NOTE}
            </p>
          )}
          {plan?.notes.map((note) => (
            <p key={note} role="status" className="ck-clip-note">
              {note}
            </p>
          ))}
          {plan !== null && plan.hidden > 0 && (
            <p role="status" className="ck-clip-note">
              {`Hid ${plan.hidden} ${plural(plan.hidden, 'line that looked', 'lines that looked')} private.`}
            </p>
          )}
          {made.guardHits > 0 && (
            <p role="status" className="ck-clip-note">
              {`${made.guardHits} ${plural(made.guardHits, 'item was', 'items were')} hidden at the last moment.`}
            </p>
          )}
        </div>
      )}

      {state.phase === 'ready' && state.saveError && (
        <p role="alert" className="ck-clip-error">
          {state.saveError}
        </p>
      )}
      {state.phase === 'saved' && (
        <p role="status" className="ck-sub">
          {`Saved to ${state.path}. Nothing was sent anywhere.`}
        </p>
      )}

      <div className="ck-sheet-actions">
        {canSave || saving ? (
          <button type="button" className="ck-btn ck-btn--primary" disabled={!canSave} onClick={() => void clip.save()}>
            {state.phase === 'saved' ? 'Save another copy…' : 'Save video…'}
          </button>
        ) : null}
        <button
          type="button"
          className="ck-btn ck-btn--secondary"
          disabled={saving}
          onClick={() => {
            if (working) clip.cancel();
            onClose();
          }}
        >
          {working ? 'Cancel' : 'Close'}
        </button>
      </div>
    </Sheet>
  );
}
