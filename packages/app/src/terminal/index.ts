export { CanvasTerminal, DEFAULT_THEME } from './canvas-terminal';
export type { CanvasTerminalOptions, TerminalTheme } from './canvas-terminal';
export { mirrorLayoutSessionKey } from './mirror-layout-session';
export { InputBatcher } from './input-batcher';
export { PaneStreamManager, MAX_BACKLOG_CHUNKS, RAW_REPLAY_LINES } from './pane-stream';
export type { PaneStream, PaneStreamMode, ReadRawBufferFn } from './pane-stream';
export { ResizeController } from './resize-controller';
export { SnapshotBatcher } from './snapshot-batcher';
export { TerminalSession } from './terminal-session';
export {
  TERMINAL_AUTHORITY_CHANGED_EVENT,
  TERMINAL_SCALE_STAGE_CLASS,
  TERMINAL_SCALE_VIEWPORT_CLASS,
  TerminalScaleController,
  computeContainerFitScale,
  createTerminalScaleMount,
} from './scaled-viewport';
export {
  TERMINAL_FONT_FAMILY,
  TERMINAL_FONT_SIZE,
  TERMINAL_SCROLLBACK,
  terminalThemeFromCss,
} from './theme';
export type {
  Disposable,
  PaneDataListener,
  PaneSnapshotListener,
  TerminalRenderMode,
  TerminalResizeHandler,
  TerminalSessionOptions,
} from './types';
export { WriteBatcher, mergeChunks, type TerminalWriter } from './write-batcher';
