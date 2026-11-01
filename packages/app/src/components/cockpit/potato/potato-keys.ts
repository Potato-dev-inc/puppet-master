// The Potato Bot page's keyboard shortcuts. Ctrl+\ is the split view's key everywhere in the cockpit, so the page
// uses Ctrl/Cmd+B for the bot list and Ctrl/Cmd+Shift+B for the right panel. They work while the page is on screen
// (the listener lives and dies with it), also while typing in the message box: they are chords, not letters.
import { useEffect } from 'react';

type KeyLike = Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>;

const chord = (e: KeyLike) => (e.ctrlKey || e.metaKey) && !(e.ctrlKey && e.metaKey) && !e.altKey;

/** Ctrl/Cmd+B: the bot list shrinks to its strip, or opens again. */
export function isListShortcut(e: KeyLike): boolean {
  return chord(e) && !e.shiftKey && e.key.toLowerCase() === 'b';
}

/** Ctrl/Cmd+Shift+B: the bot panel opens or closes. */
export function isPanelShortcut(e: KeyLike): boolean {
  return chord(e) && e.shiftKey && e.key.toLowerCase() === 'b';
}

export function usePotatoKeys(actions: { toggleList: () => void; togglePanel: () => void }): void {
  const { toggleList, togglePanel } = actions;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing) return;
      const list = isListShortcut(e);
      if (!list && !isPanelShortcut(e)) return;
      e.preventDefault();
      if (list) toggleList();
      else togglePanel();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [toggleList, togglePanel]);
}
