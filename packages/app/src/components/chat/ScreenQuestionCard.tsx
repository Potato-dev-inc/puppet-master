// "The agent is asking": the card Chat shows when a pane waits for the person and no other card answers it.
//
// Every question an agent draws in its terminal must be answerable in Chat. A profile that knows its own dialog gives a
// proper approval or question card (the caller then does not mount this one). For anything else this reads the
// screen (`lib/screen-question.ts`: numbers, a pointer, y/n; no agent is named) and offers what a hand would do in the
// terminal: pick a row, say yes or no, type an answer, press a key. "Open terminal" stays as a quiet way out.
//
// It shows only while the pane waits. The screen is read once a second and the card follows it: when the question is
// gone from the screen, so is the card.
import { useCallback, useEffect, useState, type FormEvent } from 'react';
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
  fallbackText,
  onOpenTerminal,
}: {
  paneId: string;
  /** The pane waits for the person. Nothing is shown (and nothing is read) when it does not. */
  waiting: boolean;
  /** What to say when the screen cannot be read (the question another detector found, if any). */
  fallbackText?: string | null;
  /** Switch to the Terminal tab. Without it the quiet link is not shown. */
  onOpenTerminal?: () => void;
}) {
  const [question, setQuestion] = useState<ScreenQuestion | null>(null);
  const [text, setText] = useState('');

  useEffect(() => {
    if (!waiting) {
      setQuestion(null);
      return;
    }
    let alive = true;
    let last: string | null = null;
    const read = () => {
      void tauri
        .readSnapshot(paneId)
        .then((screen) => {
          if (!alive || screen === last) return;
          last = screen;
          setQuestion(parseScreenQuestion(screen));
        })
        .catch(() => {
          if (alive) setQuestion(null);
        });
    };
    read();
    const timer = window.setInterval(read, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [paneId, waiting]);

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

  if (!waiting) return null;
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
  );
}
