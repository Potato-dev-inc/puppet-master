// "The agent is asking": the card Chat shows when a pane waits for the person and no other card answers it.
//
// Every question an agent draws in its terminal must be answerable in Chat. A profile that knows its own dialog gives a
// proper approval or question card (the caller then does not mount this one). For anything else this reads the
// screen (`lib/screen-question.ts`: numbers, a pointer, y/n; no agent is named) and offers what a hand would do in the
// terminal: pick a row, say yes or no, type an answer, press a key. "Open terminal" stays as a quiet way out.
//
// It shows while the pane waits. It is also a safety net for a pane that is not working (idle) when its screen shows a
// menu with a cursor, or when a message the person wrote has been stuck in the queue for a few seconds: no agent is
// known here, so an agent whose menus no profile reads is still answered in Chat. The screen is read about once a
// second and the card follows it: when the question is gone from the screen, so is the card.
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import type { OutboxMessage } from '../../lib/paneOutbox';
import { tauri } from '../../lib/tauri';
import {
  KEY_DOWN,
  KEY_ESC,
  KEY_TAB,
  KEY_UP,
  keysForOption,
  keysForYesNo,
  parseScreenQuestion,
  type KeyPlan,
  type ScreenQuestion,
} from '../../lib/screen-question';
import { CardButton, InboxCard } from '../cockpit/InboxCard';

const POLL_MS = 1000;
/** A pane that is not asking anything is looked at less often. */
const IDLE_POLL_MS = 1500;
/** A queued message that waited this long (ms) while the pane was not working is stuck behind something. */
export const STUCK_MS = 4000;

const NO_MESSAGES: readonly OutboxMessage[] = [];

/** Whether any message the person wrote has been waiting longer than `STUCK_MS` at `now`. */
export function hasStuckMessage(messages: readonly OutboxMessage[], now: number): boolean {
  return messages.some((m) => m.state === 'queued' && now - m.createdAt >= STUCK_MS);
}

const KEYS: ReadonlyArray<{ label: string; send: string }> = [
  { label: 'Up', send: KEY_UP },
  { label: 'Down', send: KEY_DOWN },
  { label: 'Enter', send: '' },
  { label: 'Esc', send: KEY_ESC },
  { label: 'Tab', send: KEY_TAB },
];

export function ScreenQuestionCard({
  paneId,
  waiting,
  idle = false,
  covered = false,
  queued,
  fallbackText,
  onOpenTerminal,
}: {
  paneId: string;
  /** The pane waits for the person (its status says so). */
  waiting: boolean;
  /** The pane is idle: not working, not waiting. Its screen is read for a menu with a cursor (the safety net). */
  idle?: boolean;
  /** Another card (an approval or a question) already answers this pane: nothing is shown here, nothing is read. */
  covered?: boolean;
  /** The pane's messages (the chat's outbox). One that waited a few seconds while the pane is not working is stuck. */
  queued?: readonly OutboxMessage[];
  /** What to say when the screen cannot be read (the question another detector found, if any). */
  fallbackText?: string | null;
  /** Switch to the Terminal tab. Without it the quiet link is not shown. */
  onOpenTerminal?: () => void;
}) {
  const [question, setQuestion] = useState<ScreenQuestion | null>(null);
  const [text, setText] = useState('');
  const [now, setNow] = useState(() => Date.now());
  const atRest = !covered && (waiting || idle);
  const messages = queued ?? NO_MESSAGES;
  const anyQueued = messages.some((m) => m.state === 'queued');
  useEffect(() => {
    if (!atRest || waiting || !anyQueued) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [atRest, waiting, anyQueued]);
  const stuck = atRest && !waiting && hasStuckMessage(messages, Math.max(now, Date.now()));
  // A waiting pane takes any menu or line. A pane that is only idle needs a menu with a cursor, unless a message is stuck.
  const strict = !waiting && !stuck;

  useEffect(() => {
    if (!atRest) {
      setQuestion(null);
      return;
    }
    let alive = true;
    let last: string | null = null;
    const read = () => {
      // A call that throws at once (no desktop shell) is the same as one that fails.
      void Promise.resolve()
        .then(() => tauri.readSnapshot(paneId))
        .then((screen) => {
          if (!alive || screen === last) return;
          last = screen;
          setQuestion(parseScreenQuestion(screen, { strict }));
        })
        .catch(() => {
          if (alive) setQuestion(null);
        });
    };
    read();
    const timer = window.setInterval(read, waiting ? POLL_MS : IDLE_POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [paneId, atRest, waiting, strict]);

  const send = useCallback(
    (plan: KeyPlan | null) => {
      if (!plan) return;
      // The keys first, then Enter on its own: a TUI can take text and Enter in one burst for a paste.
      void (async () => {
        if (plan.text) await tauri.writeInput(paneId, plan.text, false);
        if (plan.enter) await tauri.writeInput(paneId, '', true);
      })();
    },
    [paneId],
  );

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const answer = text.trim();
    if (!answer) return;
    void tauri.writeInput(paneId, answer, true);
    setText('');
  };

  if (!atRest) return null;
  // A pane that is only idle shows the card for a menu (or, with a stuck message, a y/n question); never for plain text.
  if (!waiting && !(question && (question.options.length >= 2 || (stuck && question.yesNo)))) return null;
  const lines = question?.lines ?? (fallbackText ? [fallbackText] : []);
  if (lines.length === 0) return null;

  const options = question?.options ?? [];
  const choices = options.map((option, index) => {
    const plan = question ? keysForOption(question, index) : null;
    if (!plan) return null;
    return (
      <CardButton
        key={`${index}-${option.label}`}
        tone={index === 0 ? 'primary' : 'secondary'}
        data-option={index}
        title={option.detail || option.label}
        onClick={() => send(plan)}
      >
        {option.label}
      </CardButton>
    );
  });

  return (
    <div className="ck-chat-dock">
      <div className="ck-dock">
        <div className="ib-dock" role="group" aria-label="Agent question">
          <InboxCard
            kind="question"
            glyph="needs"
            title="Agent is asking"
            data={{ 'data-screen-question': '' }}
            body={
              <>
                <pre className="ib-cmd" data-role="screen-lines">
                  {lines.join('\n')}
                </pre>
                <form className="ib-screen-answer" onSubmit={submit}>
                  <input
                    className="ib-screen-input"
                    type="text"
                    value={text}
                    placeholder="Type an answer"
                    aria-label="Type an answer"
                    autoComplete="off"
                    spellCheck={false}
                    onChange={(event) => setText(event.target.value)}
                  />
                  <CardButton tone="secondary" size="sm" type="submit" disabled={text.trim() === ''}>
                    Send
                  </CardButton>
                </form>
                <div className="ib-screen-keys" role="group" aria-label="Keys">
                  {KEYS.map((key) => (
                    <CardButton
                      key={key.label}
                      tone="plain"
                      size="sm"
                      data-key={key.label}
                      onClick={() => send({ text: key.send, enter: key.label === 'Enter' })}
                    >
                      {key.label}
                    </CardButton>
                  ))}
                </div>
              </>
            }
            actions={
              choices.some(Boolean) ? (
                choices
              ) : question?.yesNo ? (
                <>
                  <CardButton tone="primary" data-yesno="yes" onClick={() => send(keysForYesNo(true))}>
                    Yes
                  </CardButton>
                  <CardButton tone="secondary" data-yesno="no" onClick={() => send(keysForYesNo(false))}>
                    No
                  </CardButton>
                </>
              ) : undefined
            }
            more={
              onOpenTerminal ? (
                <CardButton tone="plain" onClick={onOpenTerminal}>
                  Open Terminal
                </CardButton>
              ) : undefined
            }
          />
        </div>
      </div>
    </div>
  );
}
