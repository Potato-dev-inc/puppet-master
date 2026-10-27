// "Remember for all agents": a small pill over text selected in a message of the Chat tab. A click (or Ctrl+Shift+M)
// adds the text as one line of the project memory every agent reads at its next start; a toast says so, with Undo.
// Esc hides the pill. Desktop only: the phone chat does not mount this.
import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import { MEMORY_OFF_PROJECT, rememberApi, rememberErrorCode, rememberErrorMessage, type RememberApi } from '../../lib/chat-remember';
import { rememberableSelection } from '../../lib/chatSelection';

export const REMEMBER_LABEL = 'Remember for all agents';
export const REMEMBER_SAVED = 'Saved. Every agent reads it at its next start.';
export const REMEMBER_ALREADY = 'Already remembered. Every agent reads it.';
export const REMEMBER_REMOVED = 'Removed from the project memory.';

/** How long a toast with Undo or Turn on stays, and a plain one. */
const ACTION_MS = 12000;
const PLAIN_MS = 3200;

interface PillAt {
  text: string;
  x: number;
  y: number;
}

interface Toast {
  message: string;
  /** The stored line Undo removes. */
  undo?: string;
  /** The text to remember again once the memory is turned on. */
  turnOn?: string;
}

export interface RememberSelectionProps {
  paneId: string;
  /** The scrolling conversation: only a selection inside it counts. */
  root: RefObject<HTMLElement | null>;
  api?: RememberApi;
}

export function RememberSelection({ paneId, root, api = rememberApi }: RememberSelectionProps) {
  const [pill, setPill] = useState<PillAt | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const pillRef = useRef(pill);
  pillRef.current = pill;
  const timer = useRef<number | null>(null);

  const flash = useCallback((next: Toast, ms: number) => {
    setToast(next);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setToast(null), ms);
  }, []);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  const remember = useCallback(
    async (text: string) => {
      setPill(null);
      try {
        const done = await api.remember(paneId, text);
        window.getSelection()?.removeAllRanges();
        if (done.added) flash({ message: REMEMBER_SAVED, undo: done.text }, ACTION_MS);
        else flash({ message: REMEMBER_ALREADY }, PLAIN_MS);
      } catch (err) {
        const code = rememberErrorCode(err);
        if (code === MEMORY_OFF_PROJECT) flash({ message: 'Shared memory is off for this project.', turnOn: text }, ACTION_MS);
        else flash({ message: rememberErrorMessage(err) }, PLAIN_MS + 800);
      }
    },
    [api, paneId, flash],
  );

  const turnOnAndRemember = useCallback(
    async (text: string) => {
      setToast(null);
      try {
        await api.setProjectEnabled(paneId, true);
      } catch (err) {
        flash({ message: rememberErrorMessage(err) }, PLAIN_MS + 800);
        return;
      }
      await remember(text);
    },
    [api, paneId, flash, remember],
  );

  const undo = useCallback(
    async (line: string) => {
      setToast(null);
      try {
        await api.forget(paneId, line);
        flash({ message: REMEMBER_REMOVED }, PLAIN_MS);
      } catch (err) {
        flash({ message: rememberErrorMessage(err) }, PLAIN_MS + 800);
      }
    },
    [api, paneId, flash],
  );

  useEffect(() => {
    const onSelection = () => {
      const found = rememberableSelection(window.getSelection(), root.current);
      if (!found) {
        setPill(null);
        return;
      }
      // Placed over the middle of the selection; a selection at the very top goes under it instead.
      const x = Math.min(Math.max(found.rect.left + found.rect.width / 2, 90), window.innerWidth - 90);
      const y = found.rect.top >= 40 ? found.rect.top - 34 : found.rect.bottom + 8;
      setPill({ text: found.text, x, y });
    };
    const hide = () => setPill(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setPill(null);
        return;
      }
      const current = pillRef.current;
      if (current && e.shiftKey && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'm') {
        e.preventDefault();
        void remember(current.text);
      }
    };
    document.addEventListener('selectionchange', onSelection);
    document.addEventListener('keydown', onKey);
    // The pill is fixed on the screen: a scroll moves the text under it, so it goes.
    window.addEventListener('scroll', hide, true);
    window.addEventListener('resize', hide);
    return () => {
      document.removeEventListener('selectionchange', onSelection);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', hide, true);
      window.removeEventListener('resize', hide);
    };
  }, [root, remember]);

  return (
    <>
      {pill && (
        <button
          type="button"
          className="ck-chat-remember"
          data-testid="remember-pill"
          style={{ left: pill.x, top: pill.y }}
          title={`${REMEMBER_LABEL} (Ctrl+Shift+M)`}
          aria-keyshortcuts="Control+Shift+M Meta+Shift+M"
          // Keeps the text selected while the pill is pressed.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void remember(pill.text)}
        >
          {REMEMBER_LABEL}
        </button>
      )}
      {toast && (
        <div className="ck-chat-remember-toast" role="status" aria-live="polite" data-testid="remember-toast">
          <span>{toast.message}</span>
          {toast.undo !== undefined && (
            <button type="button" className="ck-chat-remember-action" onClick={() => void undo(toast.undo ?? '')}>
              Undo
            </button>
          )}
          {toast.turnOn !== undefined && (
            <button type="button" className="ck-chat-remember-action" onClick={() => void turnOnAndRemember(toast.turnOn ?? '')}>
              Turn on
            </button>
          )}
        </div>
      )}
    </>
  );
}
