// Which view of an agent tab the phone shows: the terminal (the default) or the read-only chat. Kept per pane in
// localStorage, like the active tab, so a reload comes back to the same view.
import type { AgentRegistryListing } from './agentRegistry';

export type PaneView = 'terminal' | 'chat';

export const LS_PANE_VIEWS = 'pm.mobile.paneViews';

export function readPaneViews(): Record<string, PaneView> {
  try {
    const parsed = JSON.parse(localStorage.getItem(LS_PANE_VIEWS) ?? 'null') as Record<string, unknown> | null;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    const views: Record<string, PaneView> = {};
    for (const [paneId, view] of Object.entries(parsed)) if (view === 'chat') views[paneId] = 'chat';
    return views;
  } catch {
    return {};
  }
}

/** Only chats are stored: a pane with no entry shows its terminal. */
export function writePaneViews(views: Readonly<Record<string, PaneView>>): void {
  try {
    const chats = Object.fromEntries(Object.entries(views).filter(([, view]) => view === 'chat'));
    localStorage.setItem(LS_PANE_VIEWS, JSON.stringify(chats));
  } catch {
    /* private mode */
  }
}

/** The views of the panes that still exist (a closed pane's choice is forgotten). */
export function keepViewsOf(views: Readonly<Record<string, PaneView>>, paneIds: readonly string[]): Record<string, PaneView> {
  const kept = Object.entries(views).filter(([paneId]) => paneIds.includes(paneId));
  return kept.length === Object.keys(views).length ? { ...views } : Object.fromEntries(kept);
}

/** A plain shell has no agent conversation, so its tab has no Chat view. */
export function chatAllowedFor(listing: AgentRegistryListing, agentType: string): boolean {
  return listing.agents.find((agent) => agent.id === agentType)?.kind !== 'shell';
}
