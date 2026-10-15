export type DetachedCloseAction = 'kill-and-exit' | 'close';

/**
 * What the X of a detached pane window does. Panes-first: there is no grid to reattach to, so a
 * pane window just closes, and the Rust side kills the pane it showed. Worker hosts (a standalone
 * `--worker` terminal) kill their pane and quit the app.
 */
export function detachedCloseAction(opts: { workerHost: boolean }): DetachedCloseAction {
  return opts.workerHost ? 'kill-and-exit' : 'close';
}
