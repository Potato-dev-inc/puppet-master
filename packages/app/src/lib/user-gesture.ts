// The click behind an answer. Every answer the desktop UI sends to a pane prompt carries one, made
// here from the DOM event of the person's click (or key press on the focused button). The backend
// (`pane_prompt/answers.rs`) refuses a yes, and any answer to a startup gate such as folder trust,
// without a fresh, trusted gesture, and uses each gesture once. So a React effect, a re-render, a
// retry or a timer cannot answer for the person: they have no event to make a gesture from.

export interface UserGesture {
  /** Fresh per event: one gesture answers once. */
  id: string;
  /** `Event.isTrusted`: real input, not `element.click()` from code. */
  trusted: boolean;
  /** `Date.now()` when the handler ran. */
  atMs: number;
  /** The event type (`click`, `keydown`, ...). */
  kind: string;
}

/** The minimum of an event this needs (a React synthetic event or a DOM event). */
export interface GestureSource {
  isTrusted?: boolean;
  type?: string;
  nativeEvent?: { isTrusted?: boolean; type?: string };
}

function freshId(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  return `g-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

/** The gesture for the event a click handler received. */
export function gestureOf(event: GestureSource): UserGesture {
  const native = event.nativeEvent ?? event;
  return {
    id: freshId(),
    trusted: native.isTrusted === true,
    atMs: Date.now(),
    kind: native.type || event.type || 'click',
  };
}
