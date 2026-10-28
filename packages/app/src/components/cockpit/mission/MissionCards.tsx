// The mission cards in the Inbox (DESIGN-MISSIONS 4.6, 6.5) and the Missions list under them.
// - Tamper: "The checks pass, but Codex changed the tests" with up to 3 lines, then
//   [Undo the test changes and keep going] · Accept the changes (confirm sheet first).
// - Stuck: "The mission isn't getting anywhere" · [Try 5 more turns] · Hand it to me · Stop mission.
// - Setup: "The mission can't go on" · [Try again] · Stop mission.
// The budget card is the budgets module's own (it is already in the Inbox). Answers are the person's
// only: they go through `mission_decide`, never through a pane.
import { useState } from 'react';
import { isActive, tauriMissionsApi, type CardChoice, type MissionView, type MissionsApi } from '../../../lib/missions';
import { Sheet } from '../Sheet';
import { MissionGlyph, PreviewTag } from './MissionGlyph';
import { MissionProgress } from './MissionProgress';
import '../../../styles/missions.css';

function headline(m: MissionView): string {
  if (m.card_title) return m.card_title;
  switch (m.needs) {
    case 'tamper':
      return 'The checks pass, but the tests were changed';
    case 'stuck':
      return "The mission isn't getting anywhere";
    case 'setup':
      return "The mission can't go on";
    default:
      return m.state_text;
  }
}

/** "Count these tests as passing?" with what the mission will finish with. */
function acceptBody(m: MissionView): string {
  const lines = m.test_change_lines.filter((l) => !l.startsWith('and '));
  return lines.length
    ? `The mission will finish with these changes to the tests: ${lines.join('; ')}.`
    : 'The mission will finish with the tests as the agent changed them.';
}

export function MissionCard({
  m,
  api = tauriMissionsApi,
  onChanged,
  onNotify,
}: {
  m: MissionView;
  api?: MissionsApi;
  onChanged?: (m: MissionView) => void;
  onNotify?: (text: string) => void;
}) {
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const decide = (choice: CardChoice, sentence: string) => {
    setBusy(true);
    api
      .decide(m.id, choice)
      .then((next) => {
        onChanged?.(next);
        onNotify?.(sentence);
      })
      .catch((err) => onNotify?.(err instanceof Error ? err.message : String(err)))
      .finally(() => setBusy(false));
  };
  const agent = m.current_agent ?? 'The agent';
  return (
    <li className="ck-inbox-item ck-mission-card" data-mission-card={m.needs}>
      <div className="ck-inbox-top">
        <span className="ck-row-glyph">
          <MissionGlyph glyph="+" />
        </span>
        <span className="ck-row-title">{headline(m)}</span>
        <span className="ck-row-sub">{m.title}</span>
        <PreviewTag />
      </div>
      {m.needs === 'tamper' ? (
        <>
          <ul className="ck-mission-card-lines">
            {m.test_change_lines.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
          <p className="ck-q-ctx">The mission isn&apos;t done until you decide.</p>
          <div className="ck-inbox-actions">
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => decide('undo_tests', `Put the tests back. ${agent} is working again.`)}>
              Undo the test changes and keep going
            </button>
            <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => setConfirm(true)}>
              Accept the changes
            </button>
          </div>
        </>
      ) : m.needs === 'stuck' ? (
        <>
          <p className="ck-q-ctx">{m.sentence ? `${m.sentence}. ` : ''}Their notes are in MISSION.md.</p>
          <div className="ck-inbox-actions">
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => decide('more_turns', 'Trying 5 more turns.')}>
              Try 5 more turns
            </button>
            <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => decide('hand_to_me', 'The mission is yours. Its separate copy and notes are ready.')}>
              Hand it to me
            </button>
            <button type="button" className="ck-btn ck-btn--plain" disabled={busy} onClick={() => decide('stop', 'Mission stopped. Its separate copy stays.')}>
              Stop mission
            </button>
          </div>
        </>
      ) : (
        <>
          {m.sentence && <p className="ck-q-ctx">{m.sentence}</p>}
          <div className="ck-inbox-actions">
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => decide('try_again', 'Trying again.')}>
              Try again
            </button>
            <button type="button" className="ck-btn ck-btn--plain" disabled={busy} onClick={() => decide('stop', 'Mission stopped. Its separate copy stays.')}>
              Stop mission
            </button>
          </div>
        </>
      )}
      {confirm && (
        <Sheet labelledBy="ck-mission-accept-t" onClose={() => setConfirm(false)}>
          <h2 className="ck-section" id="ck-mission-accept-t">
            Count these tests as passing?
          </h2>
          <p className="ck-sub">{acceptBody(m)}</p>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={() => setConfirm(false)}>
              Cancel
            </button>
            <button
              type="button"
              className="ck-btn ck-btn--primary"
              onClick={() => {
                setConfirm(false);
                decide('accept_tests', 'Test changes accepted. The mission is done.');
              }}
            >
              Accept and finish
            </button>
          </div>
        </Sheet>
      )}
    </li>
  );
}

/** The finished missions the Inbox lists under "Ready for you" (the newest five). */
export function readyMissions(missions: MissionView[]): MissionView[] {
  return missions.filter((m) => m.state === 'green').slice(0, 5);
}

/** The Inbox's mission section: cards for missions that need you, then the missions themselves
 *  (each opens its Progress in place), finished ones under "Ready for you". `foldReady` (Focus is on)
 *  hides those finished rows. The cards that need you are never folded. */
export function MissionsSection({
  missions,
  api = tauriMissionsApi,
  onChanged,
  onOpenPane,
  onNotify,
  foldReady = false,
}: {
  missions: MissionView[];
  api?: MissionsApi;
  onChanged?: () => void;
  onOpenPane?: (paneId: string) => void;
  onNotify?: (text: string) => void;
  foldReady?: boolean;
}) {
  const [open, setOpen] = useState<string | null>(null);
  if (missions.length === 0) return null;
  const cards = missions.filter((m) => m.state === 'needs_you' && m.needs && m.needs !== 'budget');
  const working = missions.filter((m) => isActive(m));
  const ready = foldReady ? [] : readyMissions(missions);
  const row = (m: MissionView) => (
    <li key={m.id} className="ck-mission-list-item">
      <button type="button" className="ck-mission-list-row" aria-expanded={open === m.id} onClick={() => setOpen((v) => (v === m.id ? null : m.id))}>
        <MissionGlyph glyph={m.glyph} />
        <span className="ck-row-title">{m.title}</span>
        <PreviewTag />
        <span className="ck-row-sub">Mission · {m.state_text}</span>
      </button>
      {open === m.id && (
        <MissionProgress mission={m} api={api} withCard={false} onChanged={() => onChanged?.()} onOpenPane={onOpenPane} onNotify={onNotify} />
      )}
    </li>
  );
  return (
    <section className="ck-missions" aria-label="Missions">
      {cards.length > 0 && (
        <ul className="ck-list ck-q-list" aria-label="Missions that need you">
          {cards.map((m) => (
            <MissionCard key={m.id} m={m} api={api} onChanged={() => onChanged?.()} onNotify={onNotify} />
          ))}
        </ul>
      )}
      {working.length > 0 && (
        <>
          <h3 className="ck-mission-cap">Missions</h3>
          <ul className="ck-list">{working.map(row)}</ul>
        </>
      )}
      {ready.length > 0 && (
        <>
          <h3 className="ck-mission-cap">Ready for you</h3>
          <ul className="ck-list">{ready.map(row)}</ul>
        </>
      )}
    </section>
  );
}
