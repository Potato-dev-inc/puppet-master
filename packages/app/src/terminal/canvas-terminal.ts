export interface TerminalTheme {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent?: string;
  selectionBackground?: string;
  selectionForeground?: string;
  selectionInactiveBackground?: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export interface CanvasTerminalOptions {
  cols: number;
  rows: number;
  fontFamily: string;
  fontSize: number;
  scrollback: number;
  theme: TerminalTheme;
}

export const DEFAULT_THEME: TerminalTheme = {
  background: '#0a0a0a',
  foreground: '#e8e8e8',
  cursor: '#f5f5f5',
  black: '#000000',
  red: '#cd3131',
  green: '#0dbc79',
  yellow: '#e5e510',
  blue: '#2472c8',
  magenta: '#bc3fbc',
  cyan: '#11a8cd',
  white: '#e5e5e5',
  brightBlack: '#666666',
  brightRed: '#f14c4c',
  brightGreen: '#23d18b',
  brightYellow: '#f5f543',
  brightBlue: '#3b8eea',
  brightMagenta: '#d670d6',
  brightCyan: '#29b8db',
  brightWhite: '#e5e5e5',
};

/**
 * Canvas-based terminal that renders full-screen text snapshots from the
 * backend's vt100 mirror. Input is forwarded as raw keystrokes to the PTY.
 */
export class CanvasTerminal {
  private readonly container: HTMLElement;
  private readonly canvas: HTMLCanvasElement;
  private readonly ctx: CanvasRenderingContext2D;
  private readonly theme: TerminalTheme;
  private readonly fontFamily: string;
  private readonly fontSize: number;

  private cols: number;
  private rows: number;
  private snapshot = '';
  private cellWidth = 8;
  private lineHeight = 16;
  private resizeListeners = new Set<(cols: number, rows: number) => void>();
  private dataListeners = new Set<(data: string) => void>();
  private keyHandler: ((event: KeyboardEvent) => void) | null = null;
  private disposed = false;

  constructor(container: HTMLElement, options: CanvasTerminalOptions) {
    this.container = container;
    this.cols = options.cols;
    this.rows = options.rows;
    this.theme = options.theme;
    this.fontFamily = options.fontFamily;
    this.fontSize = options.fontSize;

    this.canvas = document.createElement('canvas');
    this.canvas.className = 'pm-canvas-terminal';
    this.canvas.style.display = 'block';
    this.canvas.style.width = '100%';
    this.canvas.style.height = '100%';
    container.appendChild(this.canvas);

    const ctx = this.canvas.getContext('2d');
    if (!ctx) {
      throw new Error('Canvas 2D context unavailable');
    }
    this.ctx = ctx;

    this.container.tabIndex = 0;
    this.container.style.outline = 'none';
    this.attachKeyboard();
    this.measureMetrics();
    this.resizeCanvas();
    this.render();
  }

  get dimensions(): { cols: number; rows: number } {
    return { cols: this.cols, rows: this.rows };
  }

  /** Replace the visible screen with a backend snapshot. */
  setSnapshot(text: string): void {
    this.snapshot = text;
    this.render();
  }

  /** Accept UTF-8 bytes (used by the write batcher for API compatibility). */
  write(data: Uint8Array): void {
    this.setSnapshot(new TextDecoder().decode(data));
  }

  fit(): void {
    const rect = this.container.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;

    this.measureMetrics();
    const nextCols = Math.max(1, Math.floor(rect.width / this.cellWidth));
    const nextRows = Math.max(1, Math.floor(rect.height / this.lineHeight));

    if (nextCols !== this.cols || nextRows !== this.rows) {
      this.cols = nextCols;
      this.rows = nextRows;
        this.resizeCanvas();
      for (const listener of this.resizeListeners) {
        listener(this.cols, this.rows);
      }
    }
    this.render();
  }

  onResize(listener: (cols: number, rows: number) => void): () => void {
    this.resizeListeners.add(listener);
    return () => {
      this.resizeListeners.delete(listener);
    };
  }

  onData(listener: (data: string) => void): () => void {
    this.dataListeners.add(listener);
    return () => {
      this.dataListeners.delete(listener);
    };
  }


  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    if (this.keyHandler) {
      this.container.removeEventListener('keydown', this.keyHandler);
      this.keyHandler = null;
    }
    this.resizeListeners.clear();
    this.dataListeners.clear();
    this.canvas.remove();
  }

  private attachKeyboard(): void {
    this.keyHandler = (event: KeyboardEvent) => {
      if (this.disposed) return;

      const data = keyEventToTerminalInput(event);
      if (data === null) return;

      event.preventDefault();
      for (const listener of this.dataListeners) {
        listener(data);
      }
    };
    this.container.addEventListener('keydown', this.keyHandler);
    this.container.addEventListener('pointerdown', () => {
      this.container.focus();
    });
  }

  private measureMetrics(): void {
    this.ctx.font = `${this.fontSize}px ${this.fontFamily}`;
    this.cellWidth = Math.max(1, this.ctx.measureText('M').width);
    this.lineHeight = Math.max(this.fontSize + 2, this.fontSize * 1.2);
  }

  private resizeCanvas(): void {
    const rect = this.container.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    this.canvas.width = Math.max(1, Math.floor(rect.width * dpr));
    this.canvas.height = Math.max(1, Math.floor(rect.height * dpr));
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.measureMetrics();
  }

  private render(): void {
    const rect = this.container.getBoundingClientRect();
    const width = rect.width;
    const height = rect.height;

    this.ctx.fillStyle = this.theme.background;
    this.ctx.fillRect(0, 0, width, height);

    this.ctx.fillStyle = this.theme.foreground;
    this.ctx.font = `${this.fontSize}px ${this.fontFamily}`;
    this.ctx.textBaseline = 'top';

    const lines = this.snapshotLines();
    const visible = lines.slice(-this.rows);

    for (let row = 0; row < visible.length; row += 1) {
      const line = visible[row] ?? '';
      this.ctx.fillText(line.slice(0, this.cols), 0, row * this.lineHeight + 1);
    }
  }

  private snapshotLines(): string[] {
    return this.snapshot.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n');
  }

}

function keyEventToTerminalInput(event: KeyboardEvent): string | null {
  if (event.ctrlKey || event.metaKey || event.altKey) {
    if (event.key === 'c' && event.ctrlKey) return '\x03';
    if (event.key === 'd' && event.ctrlKey) return '\x04';
    return null;
  }

  switch (event.key) {
    case 'Enter':
      return '\r';
    case 'Backspace':
      return '\x7f';
    case 'Tab':
      return '\t';
    case 'Escape':
      return '\x1b';
    case 'ArrowUp':
      return '\x1b[A';
    case 'ArrowDown':
      return '\x1b[B';
    case 'ArrowRight':
      return '\x1b[C';
    case 'ArrowLeft':
      return '\x1b[D';
    case 'Home':
      return '\x1b[H';
    case 'End':
      return '\x1b[F';
    case 'Delete':
      return '\x1b[3~';
    default:
      if (event.key.length === 1) {
        return event.key;
      }
      return null;
  }
}
