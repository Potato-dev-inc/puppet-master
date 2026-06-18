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
  /** When false, mirror PTY dimensions locally without resizing the backend (bridge/mobile). */
  syncPTYResize?: boolean;
  ptyCols?: number;
  ptyRows?: number;
}

export interface Disposable {
  dispose(): void;
}
