// Settings > Potato Bot > Recently deleted (UX-2): the notes, routines and bots deleted in the last 7 days, with
// "Bring back" for each. It opens as a sub-page, like Models and Usage. Desktop only; the list and the restore are
// `potato_trash_*` (src/lib/potato-trash.ts).
import { useCallback, useEffect, useState } from 'react';
import { plainError } from '../../../lib/potato';
import { daysLeft, tauriTrashTransport, type TrashItem, type TrashTransport } from '../../../lib/potato-trash';
import '../../../styles/cockpit-potato-trash.css';
import { Btn, SettingRow } from '../settings-ui';
import { SubPageSheet } from './PotatoSubPages';

export const TRASH_COPY = {
  label: 'Recently deleted',
  hint: 'Notes, routines and bots you deleted in the last 7 days. Bring them back here.',
  empty: 'Nothing deleted in the last 7 days.',
  loadFailed: 'Could not load Recently deleted.',
  restoreFailed: 'Could not bring that back.',
  bring: 'Bring back',
  kind: { note: 'Note', routine: 'Routine', bot: 'Bot' } as Record<string, string>,
};

export function RecentlyDeletedList({ transport = tauriTrashTransport }: { transport?: TrashTransport }) {
  const [items, setItems] = useState<TrashItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    transport.list().then(
      (list) => setItems(list),
      (e: unknown) => setError(plainError(e, TRASH_COPY.loadFailed)),
    );
  }, [transport]);

  useEffect(() => {
    load();
  }, [load]);

  const bring = (item: TrashItem) => {
    setError(null);
    transport.restore(item.id).then(load, (e: unknown) => setError(plainError(e, TRASH_COPY.restoreFailed)));
  };

  return (
    <div className="ck-potato-trash">
      {items === null && !error ? <p className="ck-sub">Loading…</p> : null}
      {items !== null && items.length === 0 ? <p className="ck-sub">{TRASH_COPY.empty}</p> : null}
      {items && items.length > 0 ? (
        <ul className="ck-potato-trash-list">
          {items.map((item) => (
            <li key={item.id} className="ck-potato-trash-row">
              <span className="ck-potato-trash-main">
                <span className="ck-potato-trash-name">{item.title}</span>
                <span className="ck-sub">
                  {TRASH_COPY.kind[item.kind] ?? 'Item'} · {daysLeft(item, Date.now())} days left
                </span>
              </span>
              <Btn aria-label={`${TRASH_COPY.bring} ${item.title}`} onClick={() => bring(item)}>
                {TRASH_COPY.bring}
              </Btn>
            </li>
          ))}
        </ul>
      ) : null}
      {error ? (
        <p className="ck-sub" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

/** The Settings row that opens the list as a sub-page. */
export function RecentlyDeletedRow({ transport = tauriTrashTransport }: { transport?: TrashTransport }) {
  const [open, setOpen] = useState(false);
  return (
    <SettingRow label={TRASH_COPY.label} description={TRASH_COPY.hint}>
      <Btn aria-label={`Open ${TRASH_COPY.label}`} onClick={() => setOpen(true)}>
        Open
      </Btn>
      {open && (
        <SubPageSheet title={TRASH_COPY.label} onClose={() => setOpen(false)}>
          <RecentlyDeletedList transport={transport} />
        </SubPageSheet>
      )}
    </SettingRow>
  );
}
