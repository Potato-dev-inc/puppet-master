import { useEffect, useState } from 'react';
import { cockpitApi, type WorktreeItem } from '../../lib/cockpit-api';

/**
 * The git worktree a pane works in (a run started with `isolation: "worktree"`, from the UI, the
 * MCP or the bridge), or `null`. It is looked up by the pane's working directory, so every pane in
 * a worktree is found no matter how it was started. Reloaded whenever the pane's status changes,
 * which is when the diff changes.
 */
export function usePaneWorktree(
  paneId: string,
  status: string,
  api: Pick<typeof cockpitApi, 'paneWorktree'> = cockpitApi,
): WorktreeItem | null {
  const [item, setItem] = useState<WorktreeItem | null>(null);

  useEffect(() => {
    setItem(null);
  }, [paneId]);

  useEffect(() => {
    let cancelled = false;
    api
      .paneWorktree(paneId)
      .then((found) => {
        if (!cancelled) setItem(found && found.exists ? found : null);
      })
      .catch(() => {
        // Not in the desktop shell, or the lookup failed: there is nothing to review.
        if (!cancelled) setItem(null);
      });
    return () => {
      cancelled = true;
    };
  }, [api, paneId, status]);

  return item;
}

/** The tab label: `Changes`, plus the number of changed files once there are some. */
export function changesTabLabel(item: WorktreeItem): string {
  const n = item.summary?.files_changed ?? 0;
  return n > 0 ? `Changes · ${n}` : 'Changes';
}
