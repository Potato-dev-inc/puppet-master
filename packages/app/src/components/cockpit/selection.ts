// Which pane the cockpit shows when the selected one is no longer in the list.
//
// A pane that was never listed is one that is still opening (a spawn, a "show this pane" request):
// it gets a grace period and the "Opening pane" screen. A pane that WAS listed and is gone was closed
// (a merge, a discard, Close, the agent exiting): showing "Opening pane" for it is wrong and flashes
// for as long as the registry takes to settle, so the view leaves it at once for the Inbox.

export function visibleSelection(
  selectedId: string | null,
  listed: { has(id: string): boolean },
  everListed: ReadonlySet<string>,
  ready: boolean,
  spawning: boolean,
): string | null {
  if (!selectedId) return null;
  if (!ready || spawning) return selectedId;
  if (listed.has(selectedId)) return selectedId;
  return everListed.has(selectedId) ? null : selectedId;
}
