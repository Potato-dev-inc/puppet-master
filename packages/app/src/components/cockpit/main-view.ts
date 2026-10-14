// The cockpit's main area shows exactly one thing. `CockpitApp` keeps that as ONE value, and every
// click is a transition on it, so "the last click wins" holds by construction (no stack of
// booleans whose render order decides).

export type MainView = 'inbox' | 'pane' | 'doctor' | 'memory' | 'spawn';

/**
 * Where the main area rests when nothing was opened on top: an agent that is starting, else the
 * selected pane, else the inbox. Used after the selection or the spawn changes, and when "Set up
 * agents" or "Project memory" is closed.
 */
export function homeView(hasSpawn: boolean, selectedId: string | null): MainView {
  if (hasSpawn) return 'spawn';
  return selectedId ? 'pane' : 'inbox';
}

/**
 * The view right after the person picks `id` (a pane, or `null` for the inbox). A running spawn
 * stays (picking a pane does not cancel it); a failed one is cleared by the selection, so it
 * no longer counts.
 */
export function viewAfterSelect(spawn: { failed: string | null } | null, id: string | null): MainView {
  const spawnStays = spawn !== null && !(id !== null && spawn.failed);
  return homeView(spawnStays, id);
}
