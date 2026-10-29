// The Collision Alert cards in the Inbox (SPEC-n33 3.9 item 1).
// - A loud alert (`clash`, `shared_folder`) is one card: title, place, detail, the file list when there is
//   more than one file, the advice, [Open <pane>] [Open <pane>] [Ignore], and the footer.
// - A quiet alert (`same_file`) sits under "No clash yet": title, detail, [Ignore], and the footer.
// The only actions are "open this pane" and "hide this alert". Nothing here can pause, stop or steer an agent.
import { useState, useEffect } from 'react';
import { isLoud, tauriCollisionApi, type CollisionApi, type CollisionView } from '../../lib/collision';
import '../../styles/cockpit-collision.css';

/** The footer of every card. Always the last line. */
export const COLLISION_FOOTER = 'Nothing was paused. You decide.';

/** The rows to show for a read. An empty read over an empty list keeps the same array, so nothing re-draws. */
function readRows(prev: CollisionView[], next: unknown): CollisionView[] {
  const list = Array.isArray(next) ? (next as CollisionView[]) : [];
  return prev.length === 0 && list.length === 0 ? prev : list;
}

/** The glyph in front of a card: loud alerts get one, quiet rows get a dash. */
function glyph(view: CollisionView): string {
  if (view.level === 'clash') return '!';
  if (view.level === 'shared_folder') return '~';
  return '-';
}

function Card({
  view,
  select,
  hasPane,
  onIgnore,
}: {
  view: CollisionView;
  select: (paneId: string) => void;
  hasPane: (paneId: string) => boolean;
  onIgnore: (id: string) => void;
}) {
  const loud = isLoud(view);
  const cls = loud ? 'ck-inbox-item ck-collision-card' : 'ck-inbox-item ck-collision-card ck-collision-quiet';
  return (
    <li className={cls} data-collision-id={view.id} data-level={view.level}>
      <div className="ck-collision-card-head">
        <span className="ck-collision-glyph" aria-hidden="true">
          {glyph(view)}
        </span>
        <p className="ck-collision-title">{view.title}</p>
      </div>
      {loud && <p className="ck-collision-place">{view.place}</p>}
      <p className="ck-collision-detail">{view.detail}</p>
      {loud && view.files.length > 1 && (
        <ul className="ck-collision-files" aria-label="Changed files">
          {view.files.map((f) => (
            <li key={f.path}>{`${f.path} - ${f.text}`}</li>
          ))}
          {view.more > 0 && <li>{`and ${view.more} more`}</li>}
        </ul>
      )}
      {loud && view.advice && <p className="ck-collision-advice">{view.advice}</p>}
      <div className="ck-collision-actions">
        {loud &&
          view.sides.map((side) =>
            hasPane(side.paneId) ? (
              <button key={side.paneId} type="button" className="ck-btn ck-btn--secondary" onClick={() => select(side.paneId)}>
                {`Open ${side.label}`}
              </button>
            ) : null,
          )}
        <button type="button" className="ck-btn ck-btn--plain" onClick={() => onIgnore(view.id)}>
          Ignore
        </button>
      </div>
      <p className="ck-collision-footer">{COLLISION_FOOTER}</p>
    </li>
  );
}

/**
 * The collision alerts in the Inbox. Renders nothing when there are none. The list is read on mount and
 * again whenever Rust says it changed. `select` opens a pane; `hasPane` says whether a pane still exists.
 */
export function CollisionSection({
  select,
  hasPane,
  api = tauriCollisionApi,
}: {
  select: (paneId: string) => void;
  hasPane: (paneId: string) => boolean;
  api?: CollisionApi;
}) {
  const [rows, setRows] = useState<CollisionView[]>([]);

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    const reload = () => {
      api
        .list()
        .then((next) => {
          if (alive) setRows((prev) => readRows(prev, next));
        })
        .catch(() => {
          if (alive) setRows((prev) => readRows(prev, []));
        });
    };
    reload();
    api
      .onChanged(reload)
      .then((off) => {
        if (alive) unlisten = off;
        else off();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [api]);

  if (rows.length === 0) return null;

  const onIgnore = (id: string) => {
    // The card leaves at once. If Rust refuses, the list is read again so the card comes back.
    setRows((prev) => prev.filter((r) => r.id !== id));
    api.ignore(id).catch(() => {
      api
        .list()
        .then((next) => setRows((prev) => readRows(prev, next)))
        .catch(() => undefined);
    });
  };

  const loud = rows.filter(isLoud);
  const quiet = rows.filter((r) => !isLoud(r));

  return (
    <section className="ck-collision" aria-label="Collision alerts">
      {loud.length > 0 && (
        <ul className="ck-list ck-collision-list" aria-label="Agents that changed the same work">
          {loud.map((v) => (
            <Card key={v.id} view={v} select={select} hasPane={hasPane} onIgnore={onIgnore} />
          ))}
        </ul>
      )}
      {quiet.length > 0 && (
        <>
          <p className="ck-collision-place" role="heading" aria-level={3}>
            No clash yet
          </p>
          <ul className="ck-list ck-collision-list" aria-label="Same file, different places">
            {quiet.map((v) => (
              <Card key={v.id} view={v} select={select} hasPane={hasPane} onIgnore={onIgnore} />
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
