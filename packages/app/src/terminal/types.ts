/** Callback invoked when new raw PTY bytes are available. */
export type PaneDataListener = (data: Uint8Array) => void;

/** A pane resize delivered in order with the pane's output (phone-sized panes). */
export type PaneResizeListener = (cols: number, rows: number) => void;

/** Subscribe to a pane's raw bytes, and optionally to its resizes in the same order. */
export type PaneDataSubscriber = (
  paneId: string,
  cb: PaneDataListener,
  onResize?: PaneResizeListener,
) => () => void;

/** @deprecated Plain snapshots are no longer used by the terminal renderer. */
export type PaneSnapshotListener = PaneDataListener;

/** Notifies the backend when the visible terminal grid size changes. */
export type TerminalResizeHandler = (cols: number, rows: number) => void;

export type TerminalRenderMode = 'owner' | 'mirror-fit-local' | 'mirror-same-grid';

export interface TerminalSessionOptions {
  paneId: string;
  onResize: TerminalResizeHandler;
  onInput: (text: string, appendNewline?: boolean) => void;
  /** When false, mirror the PTY over a bridge tunnel (orchestrator viewers). */
  syncPTYResize?: boolean;
  renderMode?: TerminalRenderMode;
  ptyCols?: number;
  ptyRows?: number;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
  disableMobileInput?: boolean;
  /**
   * The viewer sizes the pane to itself (phone-sized panes): when the keyboard opens the grid
   * gets fewer rows, so the mirror always fits its whole height and never shifts up.
   */
  sizesPane?: boolean;
  /** The agent reprints its whole conversation on every resize (`PaneInfo.reprints_on_resize`). */
  reprintsOnResize?: boolean;
}

export interface Disposable {
  dispose(): void;
}
