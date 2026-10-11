export type DetachedCloseAction = 'kill-and-exit' | 'reattach' | 'close';

/**
 * What the X of a detached pane window does. A window whose pane no longer exists has nothing
 * to reattach, so it simply closes (the default after the handler returns).
 */
export function detachedCloseAction(opts: { workerHost: boolean; paneExists: boolean }): DetachedCloseAction {
  if (opts.workerHost) return 'kill-and-exit';
  return opts.paneExists ? 'reattach' : 'close';
}
