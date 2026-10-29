// Keyboard for the Review Desk (SPEC n33 section 3.7), modelled on inbox-keys.ts. It reads the DOM the desk
// renders: `data-rd="item"` rows with `data-rd-row` (the item key). `data-rd="group"` header rows are not rows,
// so J and K skip them. The keys only move focus or call the desk's hooks; the desk decides what each action does.
//   J / K        next and previous row          Enter or O   open the row (Enter again: focus the detail)
//   Y            Keep                           F            Ask to fix
//   X            Throw away (10 s Undo)         P            open the agent's pane
//   Z            undo a pending throw, or the last "Looks good"
//   Esc          close the detail, then the desk
// Keys are ignored in inputs, contenteditable, inside any sheet or dialog, and on a button (the button keeps them).

export type ReviewAction = 'keep' | 'throw' | 'fix' | 'open';

export interface ReviewKeyHooks {
  /** Y, X, F or P on a row. The desk runs the same action its buttons would. */
  act: (action: ReviewAction, key: string) => void;
  /** Enter or O: show the row's detail, or focus it when it is already shown. */
  open: (key: string) => void;
  /** Z: undo. True when something was undone. */
  undo: () => boolean;
  /** Esc: close the detail. False when no detail was open. */
  back: () => boolean;
  /** Esc with no detail open: close the desk. */
  close: () => void;
}

/** The part of a keyboard event this needs (a DOM KeyboardEvent or a test double). */
export interface ReviewKeyLike {
  key: string;
  target: EventTarget | null;
  defaultPrevented?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  preventDefault: () => void;
}

const EDITABLE = /^(INPUT|TEXTAREA|SELECT)$/;
const OVERLAY = '.ck-sheet, [aria-modal="true"], dialog';
const BUTTON = 'button, a, [role="button"]';
export const REVIEW_ROW = '[data-rd="item"]';

const ACTIONS: Record<string, ReviewAction> = { y: 'keep', f: 'fix', x: 'throw', p: 'open' };

function isEditable(el: Element | null): boolean {
  if (!el) return false;
  return EDITABLE.test(el.tagName) || (el as HTMLElement).isContentEditable === true;
}

/** True when the key was one of ours and was handled. */
export function handleReviewKey(e: ReviewKeyLike, root: HTMLElement, hooks: ReviewKeyHooks): boolean {
  if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return false;
  const doc = root.ownerDocument;
  const target = e.target instanceof Element ? e.target : null;
  // A key typed somewhere else on the page (the chat box, the sidebar) is not the desk's.
  if (target && target !== doc.body && target !== doc.documentElement && !root.contains(target)) return false;
  if (isEditable(target) || target?.closest(OVERLAY)) return false;

  const focused = target?.closest<HTMLElement>(REVIEW_ROW) ?? null;
  const current = focused ?? root.querySelector<HTMLElement>(`${REVIEW_ROW}[aria-current="true"]`);
  const key = current?.dataset.rdRow ?? null;
  const k = e.key;
  const lower = k.toLowerCase();

  if (k === 'Escape') {
    e.preventDefault();
    if (!hooks.back()) hooks.close();
    return true;
  }
  if (lower === 'z') {
    if (!hooks.undo()) return false;
    e.preventDefault();
    return true;
  }
  // A button keeps its own keys: Enter and Space press it.
  if (target?.closest(BUTTON) && !focused) return false;

  if (lower === 'j' || lower === 'k') {
    const rows = Array.from(root.querySelectorAll<HTMLElement>(REVIEW_ROW));
    const index = current ? rows.indexOf(current) : -1;
    const next = lower === 'j' ? (index < 0 ? rows[0] : rows[index + 1]) : index <= 0 ? rows[0] : rows[index - 1];
    if (!next) return false;
    e.preventDefault();
    next.focus();
    next.scrollIntoView?.({ block: 'nearest' });
    return true;
  }

  // Everything else acts on a row, so it needs one.
  if (!key) return false;
  if (k === 'Enter' || lower === 'o') {
    e.preventDefault();
    hooks.open(key);
    return true;
  }
  const action = ACTIONS[lower];
  if (!action) return false;
  e.preventDefault();
  hooks.act(action, key);
  return true;
}
