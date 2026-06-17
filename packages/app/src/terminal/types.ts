/** Callback invoked when new raw PTY bytes are available. */
export type PaneDataListener = (data: Uint8Array) => void;

/** @deprecated Plain snapshots are no longer used by the terminal renderer. */
export type PaneSnapshotListener = PaneDataListener;

/** Notifies the backend when the visible terminal grid size changes. */
export type TerminalResizeHandler = (cols: number, rows: number) => void;

export interface TerminalSessionOptions {
  paneId: string;
  onResize: TerminalResizeHandler;
  onInput: (text: string) => void;
}

export interface Disposable {
  dispose(): void;
}
