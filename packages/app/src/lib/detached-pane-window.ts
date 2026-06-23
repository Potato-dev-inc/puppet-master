import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function detachedPaneUrl(paneId: string): string {
  const params = new URLSearchParams();
  params.set('terminal', '1');
  params.set('pane', paneId);
  params.set('detached', '1');
  return `${window.location.pathname}?${params.toString()}`;
}

function detachedPaneLabel(paneId: string): string {
  return `detached-pane-${paneId.replace(/[^a-zA-Z0-9-/:_]/g, '_')}`;
}

export interface DetachedPaneWindowSize {
  width: number;
  height: number;
}

const DEFAULT_TERMINAL_COLS = 120;
const DEFAULT_TERMINAL_ROWS = 32;
const DETACHED_CELL_WIDTH = 7.25;
const DETACHED_CELL_HEIGHT = 16;

function normalizedDetachedWindowSize(size?: DetachedPaneWindowSize): DetachedPaneWindowSize {
  const fallback = detachedWindowSizeFromGrid(DEFAULT_TERMINAL_COLS, DEFAULT_TERMINAL_ROWS);
  const width = size?.width ?? fallback.width;
  const height = size?.height ?? fallback.height;
  return {
    width: Math.max(260, Math.round(width)),
    height: Math.max(140, Math.round(height)),
  };
}

export function detachedWindowSizeFromGrid(cols?: number, rows?: number): DetachedPaneWindowSize {
  const safeCols = Number.isFinite(cols) && cols && cols > 0 ? cols : DEFAULT_TERMINAL_COLS;
  const safeRows = Number.isFinite(rows) && rows && rows > 0 ? rows : DEFAULT_TERMINAL_ROWS;
  return {
    width: Math.ceil(safeCols * DETACHED_CELL_WIDTH),
    height: Math.ceil(safeRows * DETACHED_CELL_HEIGHT),
  };
}

export async function openDetachedPaneWindow(
  paneId: string,
  title = 'Terminal pane',
  size?: DetachedPaneWindowSize,
): Promise<void> {
  const url = detachedPaneUrl(paneId);
  const windowSize = normalizedDetachedWindowSize(size);
  if (!isTauriRuntime()) {
    window.open(
      url,
      detachedPaneLabel(paneId),
      `noopener,noreferrer,width=${windowSize.width},height=${windowSize.height}`,
    );
    return;
  }

  const label = detachedPaneLabel(paneId);
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.show();
    await existing.setFocus();
    return;
  }

  const view = new WebviewWindow(label, {
    url,
    title,
    width: windowSize.width,
    height: windowSize.height,
    minWidth: 260,
    minHeight: 140,
    resizable: true,
    focus: true,
  });

  await new Promise<void>((resolve, reject) => {
    void view.once('tauri://created', () => resolve());
    void view.once('tauri://error', (event) => reject(event.payload));
  });
}
