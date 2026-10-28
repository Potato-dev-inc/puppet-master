// Package check on the approval card: one row per package the command installs, painted at once from
// the offline facts, then filled in by the public registry's answer. The answer only adds lines. It never
// moves a button, changes the risk, or takes the focus, and the row count is fixed at first paint.
// Registry text is untrusted: it is rendered as plain text only (React escapes it).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { PanePrompt } from '../../lib/cockpit-api';
import {
  REGISTRY_LABEL,
  firstPaintRows,
  packageCheckLookup,
  type PackageCheckView,
  type PackageList,
  type PaintRow,
} from '../../lib/package-check';
import { tauri } from '../../lib/tauri';
import '../../styles/package-facts.css';
import { CardButton } from './InboxCard';

/** Rows painted on the card. The rest are one line in Details. */
const CARD_ROWS = 3;

/** The part of a prompt this block reads. */
export type PackagePrompt = Pick<PanePrompt, 'promptId' | 'packages'>;

interface Answer {
  key: string;
  seq: number;
  view: PackageCheckView | null;
  failed: boolean;
}

export interface PackageFactsState {
  /** One row per package, in the first paint's order. */
  rows: PaintRow[];
  /** The backend's answer, once it arrives for this prompt. */
  view: PackageCheckView | null;
  /** Header text: "Checking npm…", "Could not check", the backend's status, or "" when nothing runs. */
  status: string;
  /** A lookup is running (first paint, or Check again). */
  busy: boolean;
  /** The lookup command failed: the first-paint rows stay. */
  failed: boolean;
  /** "Check again": drops unsettled answers on the backend and looks up again. */
  again: () => void;
}

/** "npm", "npm and PyPI", "npm, PyPI and crates.io": the registries a lookup will ask. */
function registriesOf(list: PackageList): string {
  const labels = [...new Set(list.items.filter((item) => item.lookup).map((item) => REGISTRY_LABEL[item.ecosystem]))];
  if (labels.length <= 1) return labels[0] ?? '';
  return `${labels.slice(0, -1).join(', ')} and ${labels[labels.length - 1]}`;
}

/** A first-paint summary without its "Checking…" tail, for a row that no answer will fill in. */
function withoutChecking(summary: string): string {
  return summary.replace(/(?:^|\s·\s)Checking…$/, '') || 'Not checked';
}

/**
 * The facts for one approval card. One lookup per pane and prompt; no pane (first run, tests) means no
 * lookup and only the offline rows. The rows never change count: each backend row replaces the first-paint
 * row at the same index.
 */
export function usePackageFacts(paneId: string | undefined, prompt: PackagePrompt | null | undefined): PackageFactsState {
  const promptId = prompt?.promptId ?? '';
  const list = prompt?.packages;
  const paint = useMemo(() => (list ? firstPaintRows(list) : []), [list]);
  const key = paneId && promptId && paint.length > 0 ? `${paneId}\u0000${promptId}` : '';
  const [seq, setSeq] = useState(0);
  const [answer, setAnswer] = useState<Answer | null>(null);
  const againNext = useRef(false);

  useEffect(() => {
    if (!key || !paneId) return;
    let live = true;
    const again = againNext.current;
    againNext.current = false;
    packageCheckLookup(paneId, promptId, again).then(
      (view) => {
        if (live) setAnswer({ key, seq, view, failed: false });
      },
      () => {
        if (live) setAnswer({ key, seq, view: null, failed: true });
      },
    );
    return () => {
      live = false;
    };
    // Runs once per prompt. `seq` is the only other reason: "Check again".
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, seq]);

  const current = answer && answer.key === key && answer.seq === seq ? answer : null;
  const busy = key !== '' && current === null;
  const view = current?.view ?? null;
  const failed = current?.failed ?? false;

  const rows: PaintRow[] = paint.map((row, i) => {
    const fromBackend = view?.rows[i];
    if (fromBackend) return fromBackend;
    if (row.state !== 'checking' || busy) return row;
    // No answer will come (no pane, or the command failed): the row says so instead of "Checking…".
    return { ...row, state: failed ? 'failed' : 'not_checked', summary: withoutChecking(row.summary) };
  });

  let status = '';
  if (busy) {
    const registries = list ? registriesOf(list) : '';
    status = registries ? `Checking ${registries}…` : 'Checking…';
  } else if (failed) {
    status = 'Could not check';
  } else if (view) {
    status = view.statusText;
  }

  const again = useCallback(() => {
    againNext.current = true;
    setSeq((n) => n + 1);
  }, []);

  return { rows, view, status, busy, failed, again };
}

function LookGlyph() {
  return (
    <svg className="ck-pc-glyph" viewBox="0 0 10 10" width="9" height="9" role="img" aria-label="Look closer" focusable="false">
      <path d="M5 1.5 9 8.5H1z" fill="currentColor" />
    </svg>
  );
}

function PackageLine({ row }: { row: PaintRow }) {
  return (
    <li className="ck-pc-row" data-state={row.state} data-look={row.look ? '' : undefined}>
      {row.look ? <LookGlyph /> : <span className="ck-pc-glyph ck-pc-glyph--none" aria-hidden="true" />}
      <span className="ck-pc-name">{row.name}</span>
      <span className="ck-pc-summary" aria-live="polite">
        {row.summary}
      </span>
    </li>
  );
}

function OpenPage({ url, label }: { url: string; label: string }) {
  return <CardButton tone="plain" size="sm" onClick={() => void tauri.openExternalUrl(url)}>{`Open on ${label}`}</CardButton>;
}

function PackageDetails({ rows, footer, canRetry, again }: { rows: PaintRow[]; footer: string[]; canRetry: boolean; again: () => void }) {
  return (
    <div className="ck-pc-details">
      {rows.map((row, i) => (
        <div className="ck-pc-detail" key={i}>
          <div className="ck-pc-detail-head">
            <span className="ck-pc-name">{row.name}</span>
            <span className="ck-pc-reg">{row.registryLabel}</span>
            {row.pageUrl && <OpenPage url={row.pageUrl} label={row.registryLabel} />}
          </div>
          <ul>
            {(row.lines.length > 0 ? row.lines : [{ look: row.look, text: row.summary }]).map((line, j) => (
              <li key={j}>
                {line.look && <LookGlyph />}
                <span>{line.text}</span>
              </li>
            ))}
          </ul>
        </div>
      ))}
      {footer.map((line) => (
        <p className="ck-pc-foot" key={line}>
          {line}
        </p>
      ))}
      {canRetry && (
        <CardButton tone="plain" size="sm" onClick={again}>
          Check again
        </CardButton>
      )}
    </div>
  );
}

/**
 * The Package check block. Renders nothing without packages. `details` is the card's Details toggle:
 * every row's facts, the footer lines and Check again.
 */
export function PackageFacts({
  paneId,
  prompt,
  details = false,
  variant = 'card',
}: {
  /** The pane that shows the prompt. Without it no lookup runs. */
  paneId?: string;
  prompt: PackagePrompt | null | undefined;
  details?: boolean;
  /** `bar` is the compact approval bar: smaller type, same rows. */
  variant?: 'card' | 'bar';
}) {
  const facts = usePackageFacts(paneId, prompt);
  const list = prompt?.packages;
  if (!list || (list.items.length === 0 && list.hidden === 0)) return null;

  const { rows, view } = facts;
  const looks = view ? view.look : rows.filter((row) => row.look).length;
  const outside = view?.more ?? list.more;
  const hidden = view?.hidden ?? list.hidden;
  const footer = [
    view?.footer,
    outside > 0 ? `${outside} more ${outside === 1 ? 'package was' : 'packages were'} not checked` : null,
    hidden > 0 ? `${hidden} ${hidden === 1 ? 'name looked' : 'names looked'} like a secret, so ${hidden === 1 ? 'it was' : 'they were'} not shown or checked` : null,
  ].filter((line): line is string => !!line);
  const shown = rows.slice(0, CARD_ROWS);
  const moreRows = rows.length - shown.length;

  return (
    <section className={variant === 'bar' ? 'ck-pc ck-pc--bar' : 'ck-pc'} aria-label="Package check">
      <div className="ck-pc-head">
        <span className="ck-pc-title">Package check</span>
        <span className="ck-pc-status">{looks > 0 ? `Look closer · ${looks} of ${rows.length}` : facts.status}</span>
      </div>
      {shown.length > 0 && (
        <ul className="ck-pc-rows">
          {shown.map((row, i) => (
            <PackageLine key={i} row={row} />
          ))}
        </ul>
      )}
      {moreRows > 0 && <p className="ck-pc-more">{`+${moreRows} more in Details`}</p>}
      {details && <PackageDetails rows={rows} footer={footer} canRetry={!!view?.canRetry} again={facts.again} />}
    </section>
  );
}
