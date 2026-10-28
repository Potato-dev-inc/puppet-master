// The Inbox card for a GitHub issue run (Assign an issue): "Start on #42 with Codex?" before an agent starts,
// and "Draft PR ready for #42?" before a draft pull request is opened. It is an InboxCard like every other card.
//
// The issue title and text were written by a stranger, so they are shown as plain text only: no links, no
// HTML, cut to length, and never on a button. The buttons are the choices the backend sent, in its order (the
// first one is the primary). Number keys never answer these cards: starting or publishing work needs a click.
import { useState } from 'react';
import { isIssueStart, isIssuePush, timeLeftText, type Question, type QuestionAnswer, type QuestionChoice } from '../../lib/questions';
import '../../styles/issue-card.css';
import { CardButton, InboxCard } from './InboxCard';

/** Choice ids the backend sends (Rust `issue_intake::model`). */
const START = 'start';
const PUSH = 'push';
const LOOK = 'look';
const THROW = 'throw_away';

/** The title on the card: at most this many characters (the backend cuts issue titles to 200). */
const TITLE_MAX = 200;
/** The card's text: at most this many characters (the backend's card context is 1000). */
const CONTEXT_MAX = 1000;
/** One muted line: at most this many characters. */
const LINE_MAX = 300;

const HELD = "Puppet Master won't push this. Check the copy yourself.";
/** Context lines the backend writes: the repo, who labeled and wrote it, and the text the agent reads. */
const REPO = /^Repo: (.+) \(issue #\d+\)$/;
const WHO = /^Labeled by (.+)\. Written by (.+)\.$/;
const READS = 'The agent reads the whole text';
const START_TITLE = /^Start on #(\d+) "([\s\S]*)" with .+\?$/;
const NUMBER = /#(\d+)/;

/** Cut by characters, so an emoji is never split in half. */
const cut = (text: string, max: number): string => Array.from(text).slice(0, max).join('');

/** Button words. `start` keeps the backend's own words ("Start with Codex"); the rest are the fixed copy. */
const WORDS: Record<string, string> = {
  [PUSH]: 'Open draft PR',
  [LOOK]: 'Look first',
  [THROW]: 'Throw away',
  not_now: 'Not now',
  ignore: 'Ignore this issue',
};

/**
 * What the card shows, read from the question. The question text says the number (and, for a start card,
 * the title); the context says the repo, who labeled and wrote it, and why it asks first.
 */
function readCard(q: Question) {
  const start = isIssueStart(q);
  const said = start ? START_TITLE.exec(q.question) : null;
  const number = said?.[1] ?? NUMBER.exec(q.question)?.[1] ?? null;
  const title = said ? cut(said[2], TITLE_MAX) : cut(q.question, TITLE_MAX);

  // Only the lines above "The agent reads..." are the card's own lines; the text itself is in the box.
  const all = (q.context ?? '').split('\n').map((l) => l.trim()).filter(Boolean);
  const reads = all.findIndex((l) => l.startsWith(READS));
  const head = reads < 0 ? all : all.slice(0, reads);
  const repo = head.map((l) => REPO.exec(l)?.[1]).find(Boolean) ?? null;
  const who = head.map((l) => WHO.exec(l)).find(Boolean);
  const rest = head.filter((l) => !REPO.test(l) && !WHO.test(l));
  // A push card's first line is its facts ("3 files · +40 −5 · Codex · 12 turns"); the rest are reasons.
  const facts = start ? null : rest[0] ?? null;
  const muted = (start ? rest : rest.slice(1)).map((l) => cut(l, LINE_MAX));

  const header = ['GitHub issue' + (number ? ` #${number}` : ''), repo ? cut(repo, TITLE_MAX) : null].filter(Boolean).join(' · ');
  return {
    title,
    header,
    reason: who ? `Labeled by ${cut(who[1], TITLE_MAX)} · written by ${cut(who[2], TITLE_MAX)}` : facts,
    muted,
  };
}

export function IssueCard({
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
  const [showText, setShowText] = useState(false);
  const card = readCard(q);
  // A push card with no "Open draft PR" choice is held: the backend found something it will not push.
  const held = isIssuePush(q) && !q.choices.some((c) => c.id === PUSH);
  const left = q.expires_at_ms - now;
  const shown = q.choices.filter((c) => c.id !== LOOK || onOpenPane);

  const button = (c: QuestionChoice, i: number) => {
    // The first button is the primary one. Throw away and Look first are plain buttons unless one of them comes first.
    const tone = i === 0 ? 'primary' : c.id === THROW || c.id === LOOK ? 'plain' : 'secondary';
    if (c.id === LOOK) {
      return (
        <CardButton key={c.id} tone={tone} role="open" onClick={onOpenPane}>
          {WORDS[LOOK]}
        </CardButton>
      );
    }
    const label = c.id === START ? c.label.trim() || 'Start the run' : WORDS[c.id] ?? cut(c.label, 60);
    return (
      <CardButton key={c.id} tone={tone} role="answer" onClick={() => onAnswer({ choice_id: c.id })}>
        {label}
      </CardButton>
    );
  };

  return (
    <InboxCard
      kind="issue"
      glyph="needs"
      title={card.title}
      meta={card.header}
      age={now - q.created_at_ms}
      guard
      reason={card.reason}
      caution={held ? HELD : undefined}
      note={left > 0 ? `Ends in ${timeLeftText(left)} if you do not answer.` : undefined}
      data={{
        'data-question-id': q.id,
        'data-issue-card': q.kind,
        'data-held': held ? '' : undefined,
      }}
      body={
        card.muted.length > 0 || showText ? (
          <>
            {card.muted.length > 0 ? (
              <ul className="ib-issue-notes">
                {card.muted.map((line, i) => (
                  <li key={i}>{line}</li>
                ))}
              </ul>
            ) : null}
            {showText ? (
              <pre className="ib-issue-text" aria-label="The text the agent gets">
                {cut(q.context ?? '', CONTEXT_MAX)}
              </pre>
            ) : null}
          </>
        ) : null
      }
      actions={shown.length > 0 ? <>{shown.map(button)}</> : null}
      more={
        <CardButton tone="plain" aria-expanded={showText} onClick={() => setShowText((v) => !v)}>
          {showText ? 'Hide issue text' : 'Show issue text'}
        </CardButton>
      }
    />
  );
}
