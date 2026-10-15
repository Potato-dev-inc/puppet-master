// The budget chip in a pane's header ("6 of 20 turns") and its popover. Shown only when the pane's
// run has a budget; with no budget (the default) it renders nothing. At 80% or more it goes bold,
// with no push and no card. Used up, it says so and the Inbox card takes over.
import { useEffect, useRef, useState } from 'react';
import {
  addMinutesLabel,
  addTurnsLabel,
  chipText,
  helperLine,
  tauriBudgetApi,
  useBudget,
  type BudgetAction,
  type BudgetApi,
} from '../../lib/budgets';
import '../../styles/budgets.css';

export function BudgetChip({
  paneId,
  refreshKey,
  api = tauriBudgetApi,
  onNotify,
}: {
  paneId: string;
  /** Changes when something may have moved the numbers (the pane's status). */
  refreshKey?: unknown;
  api?: BudgetApi;
  onNotify?: (message: string) => void;
}) {
  const { view, refresh } = useBudget(paneId, refreshKey, api);
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  if (!view) return null;

  const act = (action: BudgetAction) => {
    void api
      .adjust(paneId, action)
      .then((sentence) => {
        onNotify?.(sentence);
        refresh();
      })
      .catch(() => onNotify?.('Could not change the budget'));
  };
  const addTurns = addTurnsLabel(view);
  const addMinutes = addMinutesLabel(view);

  return (
    <span className="ck-budget-wrap" ref={wrap}>
      <button
        type="button"
        className={`ck-budget-chip is-${view.state}`}
        data-testid="budget-chip"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {chipText(view)}
      </button>
      {open && (
        <div className="ck-budget-pop" role="dialog" aria-label="Budget" data-testid="budget-popover">
          <div className="ck-budget-pop-head">
            <span className="ck-budget-pop-title">Budget</span>
          </div>
          <dl className="ck-budget-rows">
            {view.turns && (
              <div className="ck-budget-row">
                <dt>Turns</dt>
                <dd>{`${view.turns.used} of ${view.turns.limit}`}</dd>
              </div>
            )}
            {view.minutes && (
              <div className="ck-budget-row">
                <dt>Time</dt>
                <dd>{`${view.minutes.used} of ${view.minutes.limit} min`}</dd>
              </div>
            )}
            {view.helpers.map((helper, i) => (
              <div className="ck-budget-row ck-budget-row--helper" key={i}>
                <dt>Helper</dt>
                <dd>{helperLine(helper)}</dd>
              </div>
            ))}
          </dl>
          <p className="ck-budget-note">Pauses after the current turn. Nothing is stopped mid-way.</p>
          {view.state !== 'stopped' && (
            <div className="ck-budget-actions">
              {addTurns && (
                <button type="button" className="ck-btn ck-btn--sm ck-btn--primary ck-budget-add" onClick={() => act('add_turns')}>
                  {addTurns}
                </button>
              )}
              {addMinutes && (
                <button type="button" className="ck-btn ck-btn--plain" onClick={() => act('add_minutes')}>
                  {addMinutes}
                </button>
              )}
              <button type="button" className="ck-btn ck-btn--plain" onClick={() => act('stop')}>
                Stop here
              </button>
            </div>
          )}
        </div>
      )}
    </span>
  );
}
