// The Review Desk (SPEC n33 section 3.8): one calm list of finished work, riskiest first, in the order the
// backend gave. Rows are read-only text. The keys (review-keys.ts) and the detail's buttons drive the actions.
//   - Keep: "Looks good" (in-folder rows) runs at once with a 10 s Undo. Other rows call `onKeep`; the confirm
//     sheet belongs to the caller, so nothing is merged from here by itself.
//   - Ask to fix: calls `onFix`; the sheet belongs to the caller.
//   - Throw away: waits in a 10 s Undo window (review-desk-pending.ts) before the call is made.
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  onReviewDeskRefresh,
  reviewDeskApi,
  reviewDeskErrorText,
  type ReviewDesk as ReviewList,
  type ReviewItem,
  type ReviewLevel,
} from '../../../lib/review-desk';
import {
  pending as currentThrow,
  startThrow,
  subscribe as subscribeThrow,
  undo as undoThrow,
  UNDO_MS,
  type PendingThrow,
} from '../../../lib/review-desk-pending';
import '../../../styles/review-desk.css';
import { ReviewRow } from './ReviewRow';
import { handleReviewKey, REVIEW_ROW, type ReviewAction, type ReviewKeyHooks } from './review-keys';

/** The backend calls the desk makes. A test passes its own. */
export type ReviewDeskSlice = Pick<typeof reviewDeskApi, 'list' | 'keep' | 'throwAway' | 'unmark' | 'fix'>;

export interface ReviewDeskProps {
  api?: ReviewDeskSlice;
  /** P on a row, and the agent's pane for a detail. */
  onOpenPane?: (paneId: string) => void;
  /** Esc with no detail open. Absent: Esc does nothing. */
  onClose?: () => void;
  /** The detail of the selected row (ReviewDetail). Absent: no detail pane. */
  renderDetail?: (item: ReviewItem) => ReactNode;
  /** Y on a row that needs the confirm sheet (Merge, Night Shift, Push as draft). The sheet is the caller's. */
  onKeep?: (item: ReviewItem) => void;
  /** F on a row. The Ask to fix sheet is the caller's. */
  onFix?: (item: ReviewItem) => void;
}

/** "Looks good" on one pane, with the time its Undo ends. */
interface LookedMark {
  key: string;
  paneId: string;
  endsAt: number;
}

/** A long title in the Undo line: 40 characters at most. */
const short = (title: string): string => (title.length > 40 ? `${title.slice(0, 39)}…` : title);

const EMPTY_TEXT = 'Nothing to review. Finished work from your agents shows up here.';

export function ReviewDesk({ api = reviewDeskApi, onOpenPane, onClose, renderDetail, onKeep, onFix }: ReviewDeskProps) {
  const [list, setList] = useState<ReviewList | null>(null);
  const [loadError, setLoadError] = useState('');
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [notice, setNotice] = useState('');
  // Rows whose throw has started: hidden until the list is read again.
  const [going, setGoing] = useState<ReadonlySet<string>>(() => new Set());
  const [throwing, setThrowing] = useState<PendingThrow | null>(() => currentThrow());
  const [looked, setLooked] = useState<LookedMark | null>(null);
  const [, setTick] = useState(0);
  const rootRef = useRef<HTMLElement>(null);
  const apiRef = useRef(api);
  apiRef.current = api;
  const requestNo = useRef(0);

  const load = useCallback(async (): Promise<void> => {
    const mine = ++requestNo.current;
    try {
      const next = await apiRef.current.list();
      if (mine === requestNo.current) {
        setList(next);
        setLoadError('');
      }
    } catch (err) {
      if (mine === requestNo.current) setLoadError(reviewDeskErrorText(err));
    }
  }, []);

  useEffect(() => {
    void load();
    return onReviewDeskRefresh(() => void load());
  }, [load]);

  useEffect(() => subscribeThrow(setThrowing), []);

  // The Undo countdown ticks while a throw waits.
  useEffect(() => {
    if (!throwing) return;
    const timer = setInterval(() => setTick((n) => n + 1), 500);
    return () => clearInterval(timer);
  }, [throwing]);

  // "Marked as looked at" goes away when its Undo window ends.
  useEffect(() => {
    if (!looked) return;
    const timer = setTimeout(() => setLooked(null), Math.max(0, looked.endsAt - Date.now()));
    return () => clearTimeout(timer);
  }, [looked]);

  const items = (list?.items ?? []).filter((item) => item.key !== throwing?.key && !going.has(item.key));
  const selected = items.find((item) => item.key === selectedKey) ?? null;
  const countOf = (level: ReviewLevel): number => items.filter((item) => item.level === level).length;
  const left = throwing ? Math.max(0, Math.ceil((throwing.endsAt - Date.now()) / 1000)) : 0;

  const runThrow = async (item: ReviewItem): Promise<void> => {
    setGoing((s) => new Set(s).add(item.key));
    setNotice('');
    try {
      await apiRef.current.throwAway(item.key, item.fingerprint);
    } catch (err) {
      setNotice(reviewDeskErrorText(err));
    } finally {
      await load();
      setGoing((s) => {
        const next = new Set(s);
        next.delete(item.key);
        return next;
      });
    }
  };

  const throwItem = (item: ReviewItem): void => {
    if (item.throw.kind === 'no') {
      setNotice(item.throw.why ?? 'Throw away is off for this item.');
      return;
    }
    const start = startThrow({ key: item.key, title: item.title, fingerprint: item.fingerprint }, () => runThrow(item), setNotice);
    setNotice(start.started ? '' : start.sentence);
  };

  const keepItem = async (item: ReviewItem): Promise<void> => {
    if (item.keep.kind === 'no') {
      setNotice(item.keep.why ?? 'Keep is off for this item.');
      return;
    }
    if (item.keep.kind !== 'mark') {
      onKeep?.(item);
      return;
    }
    setNotice('');
    try {
      await apiRef.current.keep(item.key, item.fingerprint);
      if (item.paneId) setLooked({ key: item.key, paneId: item.paneId, endsAt: Date.now() + UNDO_MS });
    } catch (err) {
      setNotice(reviewDeskErrorText(err));
    }
    await load();
  };

  // Z: the pending throw first, then the last "Looks good".
  const undo = (): boolean => {
    if (undoThrow()) return true;
    if (!looked || looked.endsAt <= Date.now()) return false;
    const mark = looked;
    setLooked(null);
    apiRef.current
      .unmark(mark.paneId)
      .then(load)
      .catch((err: unknown) => setNotice(reviewDeskErrorText(err)));
    return true;
  };

  const openRow = (key: string): void => {
    if (detailOpen && key === selectedKey) {
      rootRef.current?.querySelector<HTMLElement>('[data-rd-detail]')?.focus();
      return;
    }
    setSelectedKey(key);
    setDetailOpen(true);
  };

  const act = (action: ReviewAction, key: string): void => {
    const item = items.find((i) => i.key === key);
    if (!item) return;
    if (action === 'keep') void keepItem(item);
    else if (action === 'throw') throwItem(item);
    else if (action === 'fix') {
      if (item.fixPaneId) onFix?.(item);
      else setNotice(item.fixWhyNot ?? "The agent's pane is closed.");
    } else if (item.paneId) onOpenPane?.(item.paneId);
    else setNotice("The agent's pane is closed.");
  };

  const hooks: ReviewKeyHooks = {
    act,
    open: openRow,
    undo,
    back: () => {
      if (!detailOpen) return false;
      setDetailOpen(false);
      rootRef.current?.querySelector<HTMLElement>(`${REVIEW_ROW}[aria-current="true"]`)?.focus();
      return true;
    },
    close: () => onClose?.(),
  };
  // The window listener reads the latest hooks, so a key always sees this render's state.
  const hooksRef = useRef(hooks);
  hooksRef.current = hooks;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const root = rootRef.current;
      if (root) handleReviewKey(event, root, hooksRef.current);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // Group header rows: one per group, above the first of its rows. Rows keep the backend's order.
  const rows: ReactNode[] = [];
  const headed = new Set<string>();
  for (const item of items) {
    if (item.group && !headed.has(item.group.rootPaneId)) {
      headed.add(item.group.rootPaneId);
      rows.push(
        <li key={`group-${item.group.rootPaneId}`} className="ck-rd-group" data-rd="group" role="presentation">
          {item.group.label}
        </li>,
      );
    }
    rows.push(
      <ReviewRow
        key={item.key}
        item={item}
        selected={item.key === selectedKey}
        grouped={item.group !== null}
        onFocus={() => setSelectedKey(item.key)}
        onOpen={() => {
          setSelectedKey(item.key);
          setDetailOpen(true);
        }}
      />,
    );
  }

  const showDetail = detailOpen && selected !== null && renderDetail !== undefined;
  const loaded = list !== null;

  return (
    <section className="ck-rd" ref={rootRef} aria-label="Review Desk">
      <header className="ck-rd-head">
        <h2 className="ck-rd-heading">Review · {items.length}</h2>
        <span className="ck-rd-counts">
          High risk {countOf('high')} · Check first {countOf('check')} · Low risk {countOf('low')}
        </span>
        {onClose && (
          <button type="button" className="ck-rd-btn ck-rd-close" onClick={onClose}>
            Close
          </button>
        )}
      </header>
      {list?.notes.map((note) => (
        <p key={note} className="ck-rd-note">
          {note}
        </p>
      ))}
      <div className={`ck-rd-body${showDetail ? ' is-open' : ''}`}>
        <div className="ck-rd-list-wrap">
          <ul className="ck-rd-list" aria-label="Finished work, riskiest first">
            {rows}
          </ul>
          {!loaded && !loadError && <p className="ck-rd-empty">Loading…</p>}
          {loadError && <p className="ck-rd-notice" role="alert">{loadError}</p>}
          {loaded && items.length === 0 && !throwing && !loadError && <p className="ck-rd-empty">{EMPTY_TEXT}</p>}
          {throwing && (
            <div className="ck-rd-toast" role="status">
              {`Throwing away "${short(throwing.title)}" in ${left} s · `}
              <button type="button" className="ck-rd-link" onClick={() => undoThrow()}>
                Undo (Z)
              </button>
            </div>
          )}
          {looked && (
            <div className="ck-rd-toast" role="status">
              {'Marked as looked at · '}
              <button type="button" className="ck-rd-link" onClick={() => undo()}>
                Undo (Z)
              </button>
            </div>
          )}
          {notice && (
            <p className="ck-rd-notice" role="status">
              {notice}
            </p>
          )}
        </div>
        {showDetail && selected && renderDetail && (
          <section className="ck-rd-detail" data-rd-detail tabIndex={-1} aria-label="Detail">
            {renderDetail(selected)}
          </section>
        )}
      </div>
      <p className="ck-rd-keys">
        J K move · Enter open · Y keep · F ask to fix · X throw away · Z undo · P open pane · Esc close
      </p>
    </section>
  );
}
