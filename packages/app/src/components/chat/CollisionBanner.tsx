// The Collision Alert row above a pane's chat. One row, shown only when this pane is one of the two agents in a
// clash or a shared folder (`isLoud`); a quiet "same file" alert never shows here:
//   clash:         "fake_b also changed src/auth.ts · Lines 40-80 · Open fake_b · Ignore"
//   shared folder: "fake_b is working in the same folder · Open fake_b · Ignore"
// It reads the list when it mounts and again when `collision://changed` fires. `Open` focuses the other pane through
// the callback the chat panel gives; `Ignore` hides the alert until a new file clashes. Nothing here pauses, stops or steers an agent.
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  collisionIgnore,
  collisionList,
  isLoud,
  onCollisionChanged,
  otherSide,
  type CollisionSide,
  type CollisionView,
} from '../../lib/collision';
import '../../styles/cockpit-collision.css';

/** The first alert in `views` (newest first) that is loud and names `paneId`, with the other side. */
export function pickBanner(views: CollisionView[], paneId: string): { view: CollisionView; other: CollisionSide } | null {
  for (const view of views) {
    if (!isLoud(view)) continue;
    const other = otherSide(view, paneId);
    if (other) return { view, other };
  }
  return null;
}

/**
 * The words of the row. `lead` names the other agent and what it did; `note` is the short text of the first file
 * (for example "Lines 40-80"), or null. Extra files are counted, never listed in the row.
 */
export function bannerText(view: CollisionView, other: CollisionSide): { lead: string; note: string | null } {
  const first = view.files[0];
  const extra = Math.max(0, view.files.length - 1) + view.more;
  const note = first?.text ? first.text : null;
  if (view.level === 'shared_folder') {
    return { lead: `${other.label} is working in the same folder`, note };
  }
  if (!first) return { lead: view.title, note: null };
  const more = extra > 0 ? ` and ${extra} more ${extra === 1 ? 'file' : 'files'}` : '';
  return { lead: `${other.label} also changed ${first.path}${more}`, note };
}

export function CollisionBanner({ paneId, onOpenPane }: { paneId: string; onOpenPane?: (paneId: string) => void }) {
  const [views, setViews] = useState<CollisionView[]>([]);
  const alive = useRef(true);

  const reload = useCallback(() => {
    collisionList().then(
      (next) => {
        if (alive.current) setViews(Array.isArray(next) ? next : []);
      },
      () => {
        if (alive.current) setViews([]);
      },
    );
  }, []);

  useEffect(() => {
    alive.current = true;
    let unlisten: (() => void) | undefined;
    reload();
    onCollisionChanged(reload).then(
      (stop) => {
        if (alive.current) unlisten = stop;
        else stop();
      },
      // No event channel (a browser preview, a test window): the row still shows what it loaded.
      () => {},
    );
    return () => {
      alive.current = false;
      unlisten?.();
    };
  }, [reload]);

  const hit = pickBanner(views, paneId);
  if (!hit) return null;
  const { view, other } = hit;
  const text = bannerText(view, other);

  const ignore = () => {
    // Hide at once; if the shell refuses, read the list again so the row comes back.
    setViews((prev) => prev.filter((v) => v.id !== view.id));
    collisionIgnore(view.id).catch(reload);
  };

  return (
    <div
      className="ck-collision-banner"
      role="status"
      data-collision-banner={view.level}
      data-collision-id={view.id}
    >
      <span className="ck-collision-title">{text.lead}</span>
      {text.note && (
        <>
          <span aria-hidden="true">·</span>
          <span className="ck-collision-place">{text.note}</span>
        </>
      )}
      <span className="ck-collision-actions">
        {onOpenPane && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={() => onOpenPane(other.paneId)}>
            Open {other.label}
          </button>
        )}
        <button type="button" className="ck-btn ck-btn--plain" onClick={ignore}>
          Ignore
        </button>
      </span>
    </div>
  );
}
