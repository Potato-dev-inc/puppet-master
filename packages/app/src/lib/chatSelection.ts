// Which selected text in a Chat tab can go to the project memory: text inside one message (the person's bubble or
// the agent's own text), never across messages, tool rows or activity lines.
export const REMEMBERABLE_SELECTOR = '.ck-chat-bubble, .ck-chat-text';

export interface RememberableSelection {
  /** The selected text as the person sees it (the backend cleans it). */
  text: string;
  rect: DOMRect;
}

/** The selection when it is text inside one message under `root`, else null. */
export function rememberableSelection(sel: Selection | null, root: HTMLElement | null): RememberableSelection | null {
  if (!sel || !root || sel.rangeCount === 0 || sel.isCollapsed) return null;
  const text = sel.toString();
  if (text.trim() === '') return null;
  const range = sel.getRangeAt(0);
  const common = range.commonAncestorContainer;
  const at = common.nodeType === Node.ELEMENT_NODE ? (common as Element) : common.parentElement;
  const message = at?.closest(REMEMBERABLE_SELECTOR);
  // The message must sit under `root`; a selection that starts or ends elsewhere has a common ancestor outside it.
  if (!message || !root.contains(message)) return null;
  return { text, rect: range.getBoundingClientRect() };
}
