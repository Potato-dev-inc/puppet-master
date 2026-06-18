/** Callback invoked when new raw PTY bytes are available. */
export type PaneDataListener = (data: Uint8Array) => void;

/** @deprecated Plain snapshots are no longer used by the terminal renderer. */
export type PaneSnapshotListener = PaneDataListener;

/** Notifies the backend when the visible terminal grid size changes. */
export type TerminalResizeHandler = (cols: number, rows: number) => void;

export interface TerminalSessionOptions {
  paneId: string;
  onResize: TerminalResizeHandler;
  onInput: (text: string, appendNewline?: boolean) => void;
  /** When false, mirror the PTY over a bridge tunnel (orchestrator viewers). */
  syncPTYResize?: boolean;
  ptyCols?: number;
  ptyRows?: number;
  mobileInputDelayMs?: number;
}

export interface Disposable {
  dispose(): void;
}
