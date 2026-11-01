// "The agent is asking", answerable: the card for a Chat (terminal) pane whose agent has a prompt parser. It shows the
// question the terminal shows, with the agent's own option words, one button per option (keys 1, 2, 3 for the first
// three). Nothing is preselected and nothing answers on a timer: only the person's click or key press sends an answer,
// and each answer carries that gesture (`lib/user-gesture.ts`). A stale answer means the screen changed: the card reads
// the question again and says so. When no question is on the screen, the card is gone.
import { useCallback, useEffect, useRef, useState } from 'react';
import { gestureOf, pressFor } from '../../lib/user-gesture';
import {
  answerTerminalChatPrompt,
  readTerminalChatPrompt,
  type TerminalChatOption,
  type TerminalChatPromptView,
  type TerminalChatAnswerError,
} from '../../lib/terminalChatPrompt';
import { CardButton, InboxCard } from '../cockpit/InboxCard';

/** How often the screen is read for a question (ms). Reading is not answering: no pick is made here. */
export const PROMPT_POLL_MS = 1000;
export const STALE_NOTICE = 'The question changed. Here is the new one.';
export const FAILED_NOTICE = 'That did not go through. Try again.';
/** The keys that pick the first three options. */
const KEY_OPTIONS = 3;

export function titleFor(agent: string, kind: TerminalChatPromptView['kind']): string {
  if (kind === 'trust') return `${agent} asks to trust this folder`;
  if (kind === 'permission') return `${agent} asks to run something`;
  return `${agent} asks`;
}

export function AnswerableQuestionCard({
  paneId,
  agent,
  covered = false,
  onOpenTerminal,
}: {
  paneId: string;
  /** The agent's name for the title (e.g. "Codex"). */
  agent: string;
  /** Another card already answers this pane: nothing is read or shown here. */
  covered?: boolean;
  /** Switch to the Terminal tab. Without it the quiet link is not shown. */
  onOpenTerminal?: () => void;
}) {
  const [prompt, setPrompt] = useState<TerminalChatPromptView | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const alive = useRef(true);

  const refresh = useCallback(() => {
    // A call that throws at once (no desktop shell) is the same as "no question".
    return Promise.resolve()
      .then(() => readTerminalChatPrompt(paneId))
      .then((view) => {
        if (alive.current) setPrompt(view ?? null);
      })
      .catch(() => {
        if (alive.current) setPrompt(null);
      });
  }, [paneId]);

  useEffect(() => {
    alive.current = true;
    setPrompt(null);
    setNotice(null);
    if (covered) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), PROMPT_POLL_MS);
    return () => {
      alive.current = false;
      window.clearInterval(timer);
    };
  }, [paneId, covered, refresh]);

  // Keys 1, 2, 3 press the card's buttons. The key press is the person's gesture, handed to the click it causes.
  useEffect(() => {
    if (!prompt) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      const target = event.target as HTMLElement | null;
      if (target && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))) return;
      // Only the pane the person is in answers: focus must sit inside this card or inside its pane's chat.
      // Focus on the body (no pane focused) answers nothing; the person clicks instead.
      const focused = document.activeElement;
      if (!focused || focused === document.body) return;
      const paneRoot = rootRef.current?.closest('[data-pane-id]');
      const inCard = !!rootRef.current?.contains(focused);
      const inPane = !!paneRoot && paneRoot.contains(focused) && paneRoot.getAttribute('data-pane-id') === paneId;
      if (!inCard && !inPane) return;
      const index = Number(event.key) - 1;
      if (!Number.isInteger(index) || index < 0 || index >= Math.min(KEY_OPTIONS, prompt.options.length)) return;
      const button = rootRef.current?.querySelector<HTMLButtonElement>(`[data-option="${index}"]`);
      if (!button) return;
      event.preventDefault();
      pressFor(event, () => button.click());
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [prompt]);

  const pick = (event: { isTrusted?: boolean; type?: string; nativeEvent?: { isTrusted?: boolean; type?: string } }, option: TerminalChatOption) => {
    if (!prompt) return;
    const gesture = gestureOf(event);
    const promptId = prompt.promptId;
    setNotice(null);
    answerTerminalChatPrompt(paneId, promptId, option.id, gesture).catch((error: TerminalChatAnswerError) => {
      if (!alive.current) return;
      if (error.isStale) {
        setNotice(STALE_NOTICE);
        void refresh();
      } else {
        setNotice(FAILED_NOTICE);
      }
    });
  };

  if (covered || !prompt) return null;
  const options = prompt.options;
  const lines = [prompt.text, prompt.summary].filter((s): s is string => !!s && s.trim() !== '');

  return (
    <div className="ck-chat-dock" ref={rootRef}>
      <div className="ck-dock">
        <div className="ib-dock" role="group" aria-label="Agent question">
          <InboxCard
            kind={prompt.kind === 'question' ? 'question' : 'approval'}
            glyph="needs"
            title={titleFor(agent, prompt.kind)}
            data={{ 'data-answerable-question': '', 'data-kind': prompt.kind }}
            body={
              <>
                {lines.map((line, i) => (
                  <p key={i} data-role={i === 0 ? 'prompt-text' : 'prompt-summary'}>
                    {line}
                  </p>
                ))}
                {notice && (
                  <p role="status" data-role="prompt-notice">
                    {notice}
                  </p>
                )}
              </>
            }
            actions={options.map((option, index) => (
              <CardButton
                key={`${option.id}-${index}`}
                tone="secondary"
                role="answer"
                data-option={index}
                onClick={(event) => pick(event, option)}
              >
                {index < KEY_OPTIONS && (
                  <span aria-hidden="true" data-role="key-hint">
                    {index + 1}{' '}
                  </span>
                )}
                {option.name}
              </CardButton>
            ))}
            more={
              onOpenTerminal ? (
                <CardButton tone="plain" onClick={onOpenTerminal}>
                  Open terminal
                </CardButton>
              ) : undefined
            }
          />
        </div>
      </div>
    </div>
  );
}
