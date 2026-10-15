// Keyboard for the Inbox, so a decision never needs the mouse:
//   J / K   next and previous card        1 2 3   pick that answer on the card (never on high risk or consent)
//   O       open the card's terminal      Esc     dismiss the card
// The handler reads the DOM the cards render (`data-ib-card`, `data-ib`), so every kind of card works the same.
import { focusWithRing } from './InboxCard';

const EDITABLE = /^(INPUT|TEXTAREA|SELECT)$/;
const OVERLAY = '.ck-sheet, [aria-modal="true"], dialog';

export function isEditable(el: Element | null): boolean {
  if (!el) return false;
  return EDITABLE.test(el.tagName) || (el as HTMLElement).isContentEditable === true;
}

export interface KeyLike {
  key: string;
  target: EventTarget | null;
  defaultPrevented?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  preventDefault: () => void;
}

const cardsIn = (root: HTMLElement): HTMLElement[] => Array.from(root.querySelectorAll<HTMLElement>('[data-ib-card]'));

/** True when the key was one of ours and was handled. */
export function handleInboxKey(e: KeyLike, root: HTMLElement): boolean {
  if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return false;
  const target = e.target instanceof Element ? e.target : null;
  if (isEditable(target) || target?.closest(OVERLAY)) return false;
  const cards = cardsIn(root);
  const card = target?.closest<HTMLElement>('[data-ib-card]') ?? null;
  const index = card ? cards.indexOf(card) : -1;
  const press = (selector: string, nth = 0): boolean => {
    const buttons = Array.from(card?.querySelectorAll<HTMLButtonElement>(selector) ?? []).filter((b) => !b.disabled);
    const button = buttons[nth];
    if (!button) return false;
    e.preventDefault();
    button.click();
    return true;
  };

  const key = e.key;
  if (key === 'j' || key === 'J' || key === 'k' || key === 'K') {
    const next = key.toLowerCase() === 'j' ? cards[index + 1] : index <= 0 ? cards[0] : cards[index - 1];
    if (!next) return false;
    e.preventDefault();
    focusWithRing(next);
    next.scrollIntoView?.({ block: 'nearest' });
    return true;
  }
  if (/^[1-9]$/.test(key)) {
    if (!card || card.hasAttribute('data-guard')) return false;
    return press('[data-ib="answer"]', Number(key) - 1);
  }
  if (key === 'o' || key === 'O') return press('[data-ib="open"]');
  if (key === 'Escape') return press('[data-ib="dismiss"]');
  return false;
}
