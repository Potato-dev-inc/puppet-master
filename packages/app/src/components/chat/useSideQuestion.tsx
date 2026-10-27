// The side question's two parts for one chat: the "Ask aside" button for the header and the box laid over the chat.
// Ctrl+; opens the box (Esc closes it). The shortcut is ignored while this chat is not on screen, so a hidden chat
// never answers it.
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import type { ChatTurn } from '../../lib/sessionChatModel';
import { tauriSideQuestionApi, type SideQuestionApi } from '../../lib/sideQuestion';
import { SideQuestionBox } from './SideQuestionBox';

export const SIDE_SHORTCUT_LABEL = 'Ctrl+;';

/** Ctrl+; (Cmd+; on a Mac), with no other modifier. */
export function isSideShortcut(event: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>): boolean {
  return (event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && (event.key === ';' || event.code === 'Semicolon');
}

/** False when the element or an ancestor is not displayed (a hidden tab). */
function isShown(el: Element | null): boolean {
  for (let node: Element | null = el; node; node = node.parentElement) {
    if (node instanceof HTMLElement && (node.hidden || getComputedStyle(node).display === 'none')) return false;
  }
  return el !== null;
}

export function useSideQuestion(paneId: string, turns: readonly ChatTurn[], api: SideQuestionApi = tauriSideQuestionApi): { button: ReactNode; box: ReactNode } {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => {
    setOpen(false);
    // Hand the keyboard back to the chat.
    anchor.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!isSideShortcut(event) || !isShown(anchor.current)) return;
      event.preventDefault();
      setOpen(true);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const button = (
    <button
      ref={anchor}
      type="button"
      aria-expanded={open}
      title={`Ask about this session without writing into it (${SIDE_SHORTCUT_LABEL})`}
      onClick={() => (open ? close() : setOpen(true))}
      className="rounded px-1.5 py-0.5 text-[11px] text-pm-muted ck-chat-aside"
    >
      Ask aside
    </button>
  );
  const box = open ? <SideQuestionBox paneId={paneId} api={api} turns={turns} onClose={close} /> : null;
  return { button, box };
}
