// The Inbox cards of "Red to Green" (SPEC n33 section 5, UI-2): "CI failed on #61. Ask Codex to fix it?" before an
// agent reads a failed CI log, and "Push the fix to #61?" before a fix is pushed. They are InboxCards like every
// other card.
//
// The check names, the log and the context came from a CI run or from a log written by a stranger, so they are
// shown as plain text only: no links, no HTML, no markdown, and never on a button. The buttons are the choices the
// backend sent, in its order (the first one is the primary). A push card with no "Push fix" choice is held: the
// backend found something it will not push, so this card has no push button at all. Number keys never answer these
// cards: an agent that reads a log or a push needs a click.
import { useState } from 'react';
import { ciFixLog, type FixLogView } from '../../lib/ci-fix';
import { isCiFix, isCiPush, timeLeftText, type Question, type QuestionAnswer, type QuestionChoice } from '../../lib/questions';
import '../../styles/ci-fix-card.css';
import { CardButton, InboxCard } from './InboxCard';

/** Choice ids the backend sends (Rust `ci_fix::model`). */
const FIX = 'fix';
const NOT_NOW = 'not_now';
const STOP = 'stop_watching';
const PUSH = 'push';
const LOOK = 'look';
const KEEP = 'dont_push';

/** The title on the card: at most this many characters. */
const TITLE_MAX = 200;
/** One check name: at most this many characters (the backend cuts them to 60 already). */
const CHECK_MAX = 80;
/** One muted line: at most this many characters. */
const LINE_MAX = 300;
/** A button label: at most this many characters. */
const LABEL_MAX = 60;

const HELD = "Puppet Master won't push this fix. Check the copy yourself.";

/** "CI failed on #61 (lint, test). Ask Codex to fix it?" (Rust `ci_fix::text::card_question`). */
const FIX_QUESTION = /^CI failed on #(\d+)(?: \(([\s\S]*)\))?\. (Ask [\s\S]*)$/;
const NUMBER = /#(\d+)/;
/** The lines the backend writes into the context. Anything else in the context is not shown. */
const REPO = /Repo: ([^\s/]+\/[^\s/]+)/;
const TRY = /Try (\d+) of (\d+)/;
const ROUTE = /It continues in its pane|It starts in a new pane that can't push/;
const LOG_SIZE = /Log: ([^\n]+)/;

/** Cut by characters, so an emoji is never split in half. */
const cut = (text: string, max: number): string => Array.from(text).slice(0, max).join('');

/** Button words. The fixed copy for every id but `fix`, whose label the backend builds from the agent's name. */
const WORDS: Record<string, string> = {
  [PUSH]: 'Push fix',
  [LOOK]: 'Look first',
  [KEEP]: "Don't push",
  [NOT_NOW]: 'Not now',
  [STOP]: 'Stop watching this PR',
};

/** The fix this card is about. Only Red to Green cards carry it; without it there is no Show log button. */
type CiQuestion = Question & { fix_id?: string | null };

/**
 * A fix card, read from the question and its context. The question says the PR number and the failing check
 * names; the context says the repo, the try, where the fix runs and the size of the log.
 */
function readFix(q: Question) {
  const said = FIX_QUESTION.exec(q.question.trim());
  const number = said?.[1] ?? NUMBER.exec(q.question)?.[1] ?? null;
  const context = q.context ?? '';
  const tried = TRY.exec(context);
  const route = ROUTE.exec(context)?.[0] ?? null;
  const size = LOG_SIZE.exec(context)?.[1] ?? null;
  return {
    title: said ? cut(`CI failed on #${said[1]}. ${said[3]}`, TITLE_MAX) : cut(q.question, TITLE_MAX),
    header: ['CI failed', number ? `#${number}` : null, REPO.exec(context)?.[1] ?? null].filter(Boolean).join(' · '),
    reason: [tried ? `Try ${tried[1]} of ${tried[2]}` : null, route].filter(Boolean).join(' · ') || null,
    checks: said?.[2] ? said[2].split(', ').map((name) => cut(name.trim(), CHECK_MAX)).filter(Boolean) : [],
    size: size ? `Log: ${cut(size, LINE_MAX)}` : null,
  };
}

/**
 * A push card, read from the question and its context. The question says the PR number. The context's first line
 * is the facts ("1 file · +1 −1 · 1 commit"); the lines after it are the reasons, shown muted.
 */
function readPush(q: Question) {
  const number = NUMBER.exec(q.question)?.[1] ?? null;
  const context = q.context ?? '';
  const lines = context.split('\n').map((l) => l.trim()).filter((l) => l && !REPO.test(l));
  return {
    title: cut(q.question, TITLE_MAX),
    header: ['CI fix', number ? `#${number}` : null, REPO.exec(context)?.[1] ?? null].filter(Boolean).join(' · '),
    facts: lines[0] ? cut(lines[0], LINE_MAX) : null,
    reasons: lines.slice(1).map((l) => cut(l, LINE_MAX)),
  };
}

/** The log box: the cleaned lines the agent gets, one block per failing check. Plain text only. */
function LogBox({ log, failed }: { log: FixLogView | null; failed: boolean }) {
  if (failed) return <p className="ib-ci-log-note">Couldn't load the log.</p>;
  if (!log) return <p className="ib-ci-log-note">Loading the log...</p>;
  if (log.checks.length === 0) return <p className="ib-ci-log-note">The log is gone. The fix has ended.</p>;
  return (
    <div className="ib-ci-log" aria-label="The log the agent gets">
      {log.checks.map((name) => {
        const lines = log.lines_by_check[name] ?? [];
        return (
          <div key={name} className="ib-ci-log-check">
            <div className="ib-ci-log-name">{name}</div>
            <pre className="ib-ci-log-text">{lines.length > 0 ? lines.join('\n') : 'No log lines were kept for this check.'}</pre>
          </div>
        );
      })}
      <p className="ib-ci-log-note">Snapshot {log.sha8}</p>
    </div>
  );
}

export function CiFixCard({
  q,
  now,
  onAnswer,
  onOpenPane,
}: {
  q: Question;
  now: number;
  onAnswer: (answer: QuestionAnswer) => void;
  /** Opens the run's pane for "Look first". Absent when the pane is not known: no Look first button then. */
  onOpenPane?: () => void;
}) {
  const [showLog, setShowLog] = useState(false);
  const [log, setLog] = useState<FixLogView | null>(null);
  const [failed, setFailed] = useState(false);
  // The log is read once. The card keeps it while it is shut or open again.
  const [asked, setAsked] = useState(false);

  const fixId = (q as CiQuestion).fix_id ?? null;
  const fix = isCiFix(q) ? readFix(q) : null;
  const push = fix ? null : readPush(q);
  // A push card with no "Push fix" choice is held: there is no push button at all.
  const held = isCiPush(q) && !q.choices.some((c) => c.id === PUSH);
  const left = q.expires_at_ms - now;
  const shown = q.choices.filter((c) => c.id !== LOOK || onOpenPane);

  const toggleLog = () => {
    const next = !showLog;
    setShowLog(next);
    if (next && !asked && fixId) {
      setAsked(true);
      ciFixLog(fixId)
        .then((view) => setLog(view))
        .catch(() => setFailed(true));
    }
  };

  const button = (c: QuestionChoice, i: number) => {
    // The first button is the primary one. Look first, Don't push and Stop watching are plain unless one of them comes first.
    const tone = i === 0 ? 'primary' : c.id === LOOK || c.id === KEEP || c.id === STOP ? 'plain' : 'secondary';
    if (c.id === LOOK) {
      return (
        <CardButton key={c.id} tone={tone} role="open" onClick={onOpenPane}>
          {WORDS[LOOK]}
        </CardButton>
      );
    }
    // The agent's button ("Ask Codex to fix") is the backend's own label, cut to length.
    const label = c.id === FIX ? cut(c.label.trim(), LABEL_MAX) || 'Ask the agent to fix' : WORDS[c.id] ?? cut(c.label, LABEL_MAX);
    return (
      <CardButton key={c.id} tone={tone} role="answer" onClick={() => onAnswer({ choice_id: c.id })}>
        {label}
      </CardButton>
    );
  };

  return (
    <InboxCard
      kind="ci"
      glyph="needs"
      title={fix ? fix.title : push!.title}
      meta={fix ? fix.header : push!.header}
      age={now - q.created_at_ms}
      guard
      reason={fix ? fix.reason : push!.facts}
      caution={held ? HELD : undefined}
      note={left > 0 ? `Ends in ${timeLeftText(left)} if you do not answer.` : undefined}
      data={{
        'data-question-id': q.id,
        'data-ci-card': q.kind,
        'data-held': held ? '' : undefined,
      }}
      body={
        fix ? (
          <>
            {fix.checks.length > 0 ? (
              <ul className="ib-ci-checks" aria-label="Failing checks">
                {fix.checks.map((name, i) => (
                  <li key={`${i}-${name}`}>{name}</li>
                ))}
              </ul>
            ) : null}
            {fix.size ? <p className="ib-ci-notes">{fix.size}</p> : null}
            {showLog ? <LogBox log={log} failed={failed} /> : null}
          </>
        ) : push!.reasons.length > 0 ? (
          <ul className="ib-ci-notes-list">
            {push!.reasons.map((line, i) => (
              <li key={i}>{line}</li>
            ))}
          </ul>
        ) : null
      }
      actions={shown.length > 0 ? <>{shown.map(button)}</> : null}
      more={
        fix && fixId ? (
          <CardButton tone="plain" aria-expanded={showLog} onClick={toggleLog}>
            {showLog ? 'Hide log' : 'Show log'}
          </CardButton>
        ) : null
      }
    />
  );
}
