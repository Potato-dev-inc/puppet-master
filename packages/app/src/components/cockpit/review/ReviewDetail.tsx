// The detail half of the Review Desk: the header, why the item is first, the facts, the changed files and the
// diff of one file (plain text), and the action bar. Titles, file names and diff lines are React text nodes
// only, so agent text can never run as markup.
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  levelGlyph,
  reviewDeskApi,
  reviewDeskErrorText,
  type ReviewAct,
  type ReviewDetail as ReviewDetailView,
  type ReviewFlag,
  type ReviewFileStatus,
  type ReviewItem,
  type ReviewLevel,
} from '../../../lib/review-desk';
import { lineKind, plural, splitPatch, statusMark } from '../diff';
import { KeepSheet, type ReviewKeepApi } from './KeepSheet';
import { FixSheet, type ReviewFixSheetApi } from './FixSheet';
import '../../../styles/review-detail.css';

/** The calls this view makes: the detail, Keep (also "Looks good") and Ask to fix. */
export type ReviewDetailApi = Pick<typeof reviewDeskApi, 'detail'> & ReviewKeepApi & ReviewFixSheetApi;

export interface ReviewDetailProps {
  item: ReviewItem;
  api?: ReviewDetailApi;
  /** Open an agent's pane (the row's pane, or the pane Ask to fix wrote into). */
  onOpenPane?: (paneId: string) => void;
  /** Keep is handled by the desk (it owns the sheet). Without it, this view shows its own Keep sheet. */
  onKeep?: (item: ReviewItem) => void;
  /** Keep or "Looks good" worked. The item may be gone now. */
  onKept?: (act: ReviewAct, item: ReviewItem) => void;
  /** Throw away starts the 10 s Undo window in the desk. Without it the button stays off. */
  onThrow?: (item: ReviewItem) => void;
}

/** The level of each reason's flag (the tiers of section 3.3): tests weakened and secrets are high risk. */
export const FLAG_LEVEL: Record<ReviewFlag, ReviewLevel> = {
  tests_weakened: 'high',
  sensitive: 'high',
  checks_failed: 'check',
  tests_changed: 'check',
  packages_changed: 'check',
  big: 'check',
  unknown: 'check',
};

const STATUS_WORD: Record<ReviewFileStatus, 'added' | 'modified' | 'deleted'> = {
  A: 'added',
  M: 'modified',
  D: 'deleted',
};

const MINUS = '−';
export const LOOKED_TEXT = 'Marked as looked at · Undo (Z)';
export const NO_PANE_TEXT = "The agent's pane is closed.";
const CHANGED_TEXT = 'This changed since you looked. Look again.';

/** `+1,204 −88`, with a real minus sign, like the Copies tab. */
export function countsText(added: number, deleted: number): string {
  return `+${added.toLocaleString('en-US')} ${MINUS}${deleted.toLocaleString('en-US')}`;
}

/** Clock time of when the item finished, 24-hour, like "08:14". */
export function finishedText(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** Each unique muted line that says why a button is off. */
function offReasons(item: ReviewItem): string[] {
  const out: string[] = [];
  const add = (text: string | null | undefined) => {
    if (text && !out.includes(text)) out.push(text);
  };
  if (item.keep.kind === 'no') add(item.keep.why);
  if (item.throw.kind === 'no') add(item.throw.why);
  if (!item.fixPaneId) add(item.fixWhyNot ?? NO_PANE_TEXT);
  if (!item.paneId) add(NO_PANE_TEXT);
  return out;
}

export function ReviewDetail({ item, api = reviewDeskApi, onOpenPane, onKeep, onKept, onThrow }: ReviewDetailProps) {
  const [detail, setDetail] = useState<ReviewDetailView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [sheet, setSheet] = useState<'keep' | 'fix' | null>(null);
  const [looked, setLooked] = useState(false);
  const [actError, setActError] = useState<string | null>(null);
  // A ref, not state: a second press in the same tick must not mark twice.
  const markingRef = useRef(false);

  // A new item (or a new fingerprint) reloads the patch and drops the sheet and the local marks.
  useEffect(() => {
    let live = true;
    setDetail(null);
    setLoadError(null);
    setPicked(null);
    setSheet(null);
    setLooked(false);
    setActError(null);
    api.detail(item.key).then(
      (view) => {
        if (live) setDetail(view);
      },
      (err) => {
        if (live) setLoadError(reviewDeskErrorText(err));
      },
    );
    return () => {
      live = false;
    };
  }, [item.key, item.fingerprint, api]);

  const patchFiles = useMemo(() => (detail ? splitPatch(detail.patch) : new Map<string, string[]>()), [detail]);
  const rows = item.files.rows;
  const current = picked ?? rows[0]?.path ?? [...patchFiles.keys()][0] ?? null;
  const lines = current ? (patchFiles.get(current) ?? null) : null;
  const currentRow = rows.find((row) => row.path === current) ?? null;
  const extra = detail?.extra ?? [];
  const stale = detail !== null && detail.item.fingerprint !== item.fingerprint;

  const keepOff = item.keep.kind === 'no';
  const throwOff = item.throw.kind === 'no' || !onThrow;
  const fixOff = !item.fixPaneId;
  const openOff = !item.paneId;
  const whys = offReasons(item);

  const markLooked = async () => {
    if (markingRef.current) return;
    markingRef.current = true;
    setActError(null);
    try {
      const act = await api.keep(item.key, item.fingerprint);
      setLooked(true);
      onKept?.(act, item);
    } catch (err) {
      setActError(reviewDeskErrorText(err));
    } finally {
      markingRef.current = false;
    }
  };

  const onKeepClick = () => {
    if (keepOff) return;
    if (item.keep.kind === 'mark') {
      void markLooked();
      return;
    }
    if (onKeep) {
      onKeep(item);
      return;
    }
    setSheet('keep');
  };

  return (
    <section className="ck-rd-detail" aria-label="Review item">
      <header className="ck-rd-detail-head">
        <h2 className="ck-rd-detail-title">{item.title}</h2>
        <p className="ck-rd-detail-meta">
          {[
            item.agentLabel,
            item.whereLabel,
            `finished ${finishedText(item.finishedAtMs)}`,
            item.turns > 0 ? plural(item.turns, 'turn') : null,
          ]
            .filter(Boolean)
            .join(' · ')}
        </p>
        <p className="ck-rd-detail-level">
          <span className="ck-rd-detail-glyph" aria-hidden="true">
            {levelGlyph(item.level)}
          </span>
          {item.levelWord}
        </p>
        {item.group && <p className="ck-rd-detail-group">{item.group.label}</p>}
      </header>

      <section className="ck-rd-detail-block" aria-label="Why it is first">
        <h3 className="ck-rd-detail-h">Why it is first</h3>
        <ul className="ck-rd-detail-reasons">
          {item.reasons.length === 0 && <li>No flags</li>}
          {item.reasons.map((reason, i) => (
            <li key={`${reason.flag}-${i}`}>
              <span className="ck-rd-detail-glyph" aria-hidden="true">
                {levelGlyph(FLAG_LEVEL[reason.flag])}
              </span>
              {reason.sentence}
            </li>
          ))}
        </ul>
      </section>

      <dl className="ck-rd-detail-facts">
        <dt>Where</dt>
        <dd>{item.whereLabel}</dd>
        <dt>Files</dt>
        <dd>
          {plural(item.files.total, 'file')} {countsText(item.files.added, item.files.deleted)}
        </dd>
      </dl>

      {extra.map((line, i) => (
        <p key={i} className="ck-rd-detail-extra">
          {line}
        </p>
      ))}
      {stale && <p className="ck-rd-sheet-error">{CHANGED_TEXT}</p>}
      {loadError && <p className="ck-rd-detail-why">{`The diff could not be loaded. ${loadError}`}</p>}

      <section className="ck-rd-detail-block" aria-label="Changed files">
        <h3 className="ck-rd-detail-h">Files</h3>
        <ul className="ck-rd-detail-files">
          {rows.map((row) => {
            const word = STATUS_WORD[row.status];
            return (
              <li key={row.path}>
                <button
                  type="button"
                  className="ck-rd-detail-file"
                  aria-pressed={row.path === current}
                  onClick={() => setPicked(row.path)}
                >
                  <span className="ck-rd-detail-mark" aria-label={word}>
                    {statusMark(word)}
                  </span>
                  <span className="ck-rd-detail-path">{row.path}</span>
                  <span className="ck-rd-detail-counts">{row.binary ? 'binary' : countsText(row.added, row.deleted)}</span>
                </button>
              </li>
            );
          })}
        </ul>
        {item.files.total > rows.length && (
          <p className="ck-rd-detail-why">{`${item.files.total - rows.length} more files are not listed.`}</p>
        )}
      </section>

      <section className="ck-rd-detail-block" aria-label="Diff">
        <h3 className="ck-rd-detail-h">{current ? `Diff · ${current}` : 'Diff'}</h3>
        {!detail && !loadError && <p className="ck-rd-detail-why">Loading the diff.</p>}
        {detail && lines === null && (
          <p className="ck-rd-detail-why">
            {currentRow?.binary ? 'A binary file. No text diff.' : 'No text diff for this file.'}
          </p>
        )}
        {lines && (
          <pre className="ck-rd-detail-diff" aria-label="Diff, plain text, secrets masked">
            {lines.map((line, i) => (
              <span key={i} className={`ck-rd-detail-line ck-rd-detail-${lineKind(line)}`}>
                {line || ' '}
              </span>
            ))}
          </pre>
        )}
        {detail?.patchTruncated && <p className="ck-rd-detail-why">The diff is cut. Open the pane to see all of it.</p>}
      </section>

      <div className="ck-rd-detail-actions">
        <button
          type="button"
          className="ck-btn ck-btn--primary ck-btn--sm"
          disabled={keepOff}
          title={keepOff ? item.keep.why ?? undefined : undefined}
          onClick={onKeepClick}
        >
          {item.keep.label}
          <kbd className="ck-rd-detail-key">Y</kbd>
        </button>
        <button
          type="button"
          className="ck-btn ck-btn--secondary ck-btn--sm"
          disabled={fixOff}
          title={fixOff ? item.fixWhyNot ?? NO_PANE_TEXT : undefined}
          onClick={() => setSheet('fix')}
        >
          Ask to fix
          <kbd className="ck-rd-detail-key">F</kbd>
        </button>
        <button
          type="button"
          className="ck-btn ck-btn--secondary ck-btn--sm"
          disabled={throwOff}
          title={throwOff ? item.throw.why ?? undefined : undefined}
          onClick={() => onThrow?.(item)}
        >
          Throw away
          <kbd className="ck-rd-detail-key">X</kbd>
        </button>
        <button
          type="button"
          className="ck-btn ck-btn--plain ck-btn--sm"
          disabled={openOff}
          title={openOff ? NO_PANE_TEXT : undefined}
          onClick={() => item.paneId && onOpenPane?.(item.paneId)}
        >
          Open pane
          <kbd className="ck-rd-detail-key">P</kbd>
        </button>
      </div>
      {looked && <p className="ck-rd-detail-why">{LOOKED_TEXT}</p>}
      {actError && <p className="ck-rd-sheet-error" role="alert">{actError}</p>}
      {whys.map((text) => (
        <p key={text} className="ck-rd-detail-why">
          {text}
        </p>
      ))}

      {sheet === 'keep' && (
        <KeepSheet
          item={item}
          extra={extra}
          api={api}
          onCancel={() => setSheet(null)}
          onDone={(act) => {
            setSheet(null);
            onKept?.(act, item);
          }}
        />
      )}
      {sheet === 'fix' && (
        <FixSheet
          item={item}
          api={api}
          onCancel={() => setSheet(null)}
          onDone={(paneId) => {
            setSheet(null);
            onOpenPane?.(paneId);
          }}
        />
      )}
    </section>
  );
}
