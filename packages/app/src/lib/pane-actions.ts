// Pane actions that live in the pane's own view (rename, reflow, copy selection, details, close) but are asked
// for from elsewhere: the pane row's right-click menu in the sidebar and the command palette. The PaneView of
// that pane runs them. When the pane is not the open one yet it is selected first, so the request waits here
// until its view mounts.

export type PaneAction = 'rename' | 'reflow' | 'copy' | 'details' | 'close';

const EVENT = 'pm:pane-action';
/** A request nobody picked up in this time (its pane never opened) is dropped. */
export const PANE_ACTION_TTL_MS = 3000;

let pending: { paneId: string; action: PaneAction; at: number } | null = null;

/** Ask the view of `paneId` to run `action`. Select the pane before or after: it runs once the view is there. */
export function requestPaneAction(paneId: string, action: PaneAction, now: number = Date.now()): void {
  pending = { paneId, action, at: now };
  window.dispatchEvent(new CustomEvent(EVENT, { detail: { paneId, action } }));
}

/** The action waiting for the view of `paneId`, if any (and still fresh). Taking it clears it. */
export function takePendingPaneAction(paneId: string, now: number = Date.now()): PaneAction | null {
  const p = pending;
  if (!p || p.paneId !== paneId) return null;
  pending = null;
  return now - p.at <= PANE_ACTION_TTL_MS ? p.action : null;
}

/** Subscribe the view of `paneId`: runs the pending action at once (a view that just mounted) and each later request. */
export function subscribePaneActions(paneId: string, run: (action: PaneAction) => void): () => void {
  const waiting = takePendingPaneAction(paneId);
  if (waiting) run(waiting);
  const onEvent = (e: Event) => {
    const d = (e as CustomEvent<{ paneId: string; action: PaneAction }>).detail;
    if (!d || d.paneId !== paneId) return;
    takePendingPaneAction(paneId);
    run(d.action);
  };
  window.addEventListener(EVENT, onEvent);
  return () => window.removeEventListener(EVENT, onEvent);
}
