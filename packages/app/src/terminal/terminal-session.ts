import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { InputBatcher } from './input-batcher';
import {
  configureXtermTextareaForMobileMirror,
  isMobileInputDevice,
  MobileInputGuard,
  type MobileInputDelivery,
} from './mobile-input-guard';
import { MirrorEchoFilter } from './mirror-echo-filter';
import { applyMirrorLocalEcho } from './mirror-local-echo';
import {
  TERMINAL_FONT_FAMILY,
  TERMINAL_FONT_SIZE,
  TERMINAL_SCROLLBACK,
  terminalThemeFromCss,
} from './theme';
import type { PaneDataListener, TerminalSessionOptions } from './types';
import '@xterm/xterm/css/xterm.css';

function isWindowsRuntime(): boolean {
  return navigator.userAgent.includes('Windows');
}

function openExternalUrl(uri: string): void {
  window.open(uri, '_blank', 'noopener,noreferrer');
}

/**
 * Owns one xterm.js instance for a pane: raw PTY rendering, resize, input
 * batching, reattach replay, and StrictMode-safe deferred open.
 */
export class TerminalSession {
  private term: Terminal | null = null;
  private fitAddon: FitAddon | null = null;
  private inputBatcher: InputBatcher | null = null;
  private unlistenData: (() => void) | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private intersectionObserver: IntersectionObserver | null = null;
  private imeObserver: MutationObserver | null = null;
  private mobileInputGuard: MobileInputGuard | null = null;
  private focusCleanup: (() => void) | null = null;
  private openFrame: number | null = null;
  private resizeFrame: number | null = null;
  private refreshFrame: number | null = null;
  private lastSentCols = 0;
  private lastSentRows = 0;
  private ptyCols: number;
  private ptyRows: number;
  private readonly syncPTYResize: boolean;
  private readonly mirrorPTY: boolean;
  private muteOnData = false;
  private readonly mirrorEchoFilter = new MirrorEchoFilter();
  private generation = 0;
  private themeObserver: MutationObserver | null = null;
  private disposed = false;

  constructor(private readonly options: TerminalSessionOptions) {
    this.syncPTYResize = options.syncPTYResize ?? true;
    this.mirrorPTY = !this.syncPTYResize;
    this.ptyCols = options.ptyCols ?? 80;
    this.ptyRows = options.ptyRows ?? 24;
  }

  setPtyDimensions(cols: number, rows: number): void {
    if (cols <= 0 || rows <= 0) return;
    this.ptyCols = cols;
    this.ptyRows = rows;
    this.applyPtyDimensions();
  }

  mount(
    container: HTMLElement,
    subscribePaneData: (paneId: string, cb: PaneDataListener) => () => void,
  ): void {
    const gen = ++this.generation;
    this.disposed = false;

    this.openFrame = requestAnimationFrame(() => {
      this.openFrame = null;
      if (this.disposed || gen !== this.generation || !container.isConnected) {
        return;
      }

      const mobileMirror = this.mirrorPTY && isMobileInputDevice();
      const terminal = new Terminal({
        theme: terminalThemeFromCss(),
        fontFamily: TERMINAL_FONT_FAMILY,
        fontSize: TERMINAL_FONT_SIZE,
        scrollback: TERMINAL_SCROLLBACK,
        cursorBlink: true,
        convertEol: false,
        allowProposedApi: true,
        allowTransparency: false,
        scrollOnUserInput: true,
        disableStdin: mobileMirror,
        windowsPty: isWindowsRuntime()
          ? {
              backend: 'conpty',
            }
          : undefined,
      });

      const fitAddon = new FitAddon();
      const unicode11Addon = new Unicode11Addon();
      const webLinksAddon = new WebLinksAddon((_event, uri) => {
        openExternalUrl(uri);
      });

      terminal.loadAddon(fitAddon);
      terminal.loadAddon(webLinksAddon);
      terminal.loadAddon(unicode11Addon);
      terminal.unicode.activeVersion = '11';
      terminal.open(container);

      this.term = terminal;
      this.fitAddon = fitAddon;
      this.inputBatcher = new InputBatcher((text) => {
        this.options.onInput(text);
      }, 4);

      terminal.onData((data) => {
        if (this.muteOnData) return;
        if (mobileMirror) {
          this.mirrorEchoFilter.noteOutbound(data);
          applyMirrorLocalEcho(terminal, data);
        }
        this.inputBatcher?.push(data);
        this.scrollToCursor();
        this.scheduleRefresh();
      });

      this.unlistenData = subscribePaneData(this.options.paneId, (data) => {
        if (
          this.mirrorPTY &&
          isMobileInputDevice() &&
          this.mirrorEchoFilter.shouldSkipInbound(data)
        ) {
          return;
        }
        terminal.write(data, () => {
          this.scrollToCursor();
          this.scheduleRefresh();
        });
      });

      this.installFileLinkProvider(terminal);
      this.installTextareaGuards(container, terminal, this.inputBatcher);
      this.installResizeObservers(container);
      this.installFocusHandlers(container);
      this.installThemeObserver(terminal);
      if (this.syncPTYResize) {
        this.fitAndNotify();
      } else {
        this.applyPtyDimensions();
      }
      this.scheduleRefresh();
    });
  }

  dispose(): void {
    this.disposed = true;
    this.generation += 1;

    if (this.openFrame !== null) {
      cancelAnimationFrame(this.openFrame);
      this.openFrame = null;
    }
    if (this.resizeFrame !== null) {
      cancelAnimationFrame(this.resizeFrame);
      this.resizeFrame = null;
    }
    if (this.refreshFrame !== null) {
      cancelAnimationFrame(this.refreshFrame);
      this.refreshFrame = null;
    }

    this.unlistenData?.();
    this.unlistenData = null;

    this.resizeObserver?.disconnect();
    this.resizeObserver = null;

    this.intersectionObserver?.disconnect();
    this.intersectionObserver = null;

    this.imeObserver?.disconnect();
    this.imeObserver = null;

    this.mobileInputGuard?.dispose();
    this.mobileInputGuard = null;

    this.themeObserver?.disconnect();
    this.themeObserver = null;

    this.focusCleanup?.();
    this.focusCleanup = null;

    this.inputBatcher?.dispose();
    this.inputBatcher = null;

    this.fitAddon = null;
    this.term?.dispose();
    this.term = null;
  }

  private installResizeObservers(container: HTMLElement): void {
    this.resizeObserver = new ResizeObserver(() => {
      this.scheduleFit();
    });
    this.resizeObserver.observe(container);

    this.intersectionObserver = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          window.setTimeout(() => {
            if (this.syncPTYResize) {
              this.fitAndNotify();
              this.refreshAndNudgeAltBuffer();
            } else {
              this.applyPtyDimensions();
              this.scheduleRefresh();
            }
          }, 50);
        }
      },
      { threshold: 0.1 },
    );
    this.intersectionObserver.observe(container);
  }

  private installThemeObserver(terminal: Terminal): void {
    const applyTheme = () => {
      terminal.options.theme = terminalThemeFromCss();
      this.scheduleRefresh();
    };
    this.themeObserver = new MutationObserver(applyTheme);
    this.themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });
  }

  private installFocusHandlers(container: HTMLElement): void {
    const focusTerminal = () => {
      if (this.mobileInputGuard) {
        this.mobileInputGuard.focus();
        window.setTimeout(() => this.mobileInputGuard?.focus(), 0);
        return;
      }
      this.term?.focus();
    };
    container.addEventListener('pointerdown', focusTerminal);
    this.focusCleanup = () => {
      container.removeEventListener('pointerdown', focusTerminal);
    };
  }

  private installTextareaGuards(
    container: HTMLElement,
    terminal: Terminal,
    inputBatcher: InputBatcher,
  ): void {
    const textarea = container.querySelector('.xterm-helper-textarea') as HTMLTextAreaElement | null;
    const mobileMirror = this.mirrorPTY && isMobileInputDevice();

    const fixImePosition = () => {
      if (!textarea) return;
      if (mobileMirror) {
        configureXtermTextareaForMobileMirror(textarea);
        return;
      }
      textarea.style.position = 'fixed';
      textarea.style.bottom = '80px';
      textarea.style.left = '220px';
      textarea.style.top = 'auto';
      textarea.style.width = '1px';
      textarea.style.height = '20px';
      textarea.style.opacity = '0';
      textarea.style.zIndex = '10';
    };

    if (textarea) {
      this.imeObserver = new MutationObserver(fixImePosition);
      this.imeObserver.observe(textarea, { attributes: true, attributeFilter: ['style'] });
    }
    fixImePosition();

    if (mobileMirror) {
      const emitMobileInput = (text: string, delivery: MobileInputDelivery) => {
        if (!text) return;
        if (delivery === 'immediate') {
          inputBatcher.flushNow();
        }

        this.mirrorEchoFilter.noteOutbound(text);
        this.muteOnData = true;
        try {
          applyMirrorLocalEcho(terminal, text);
        } finally {
          this.muteOnData = false;
        }

        if (delivery === 'immediate') {
          this.options.onInput(text);
        } else {
          inputBatcher.push(text);
        }
        this.scrollToCursor();
        this.scheduleRefresh();
      };

      this.mobileInputGuard = new MobileInputGuard({
        container,
        emitInput: emitMobileInput,
        scrollToCursor: () => this.scrollToCursor(),
      });
    }
  }

  private scrollToCursor(): void {
    const terminal = this.term;
    if (!terminal) return;
    terminal.scrollToBottom();
  }

  private installFileLinkProvider(terminal: Terminal): void {
    terminal.registerLinkProvider({
      provideLinks(bufferLineNumber, callback) {
        const line = terminal.buffer.active.getLine(bufferLineNumber - 1);
        if (!line) {
          callback(undefined);
          return;
        }

        const text = line.translateToString();
        const fileUrlRegex = /file:\/\/\/[^\s'"\])}>,;`]+/g;
        const links = [];
        let match: RegExpExecArray | null;
        while ((match = fileUrlRegex.exec(text)) !== null) {
          const url = match[0];
          links.push({
            text: url,
            range: {
              start: { x: match.index + 1, y: bufferLineNumber },
              end: { x: match.index + url.length, y: bufferLineNumber },
            },
            activate() {
              openExternalUrl(url);
            },
          });
        }
        callback(links.length > 0 ? links : undefined);
      },
    });
  }

  private scheduleFit(): void {
    if (!this.syncPTYResize) {
      this.applyPtyDimensions();
      return;
    }
    if (this.resizeFrame !== null) return;
    this.resizeFrame = requestAnimationFrame(() => {
      this.resizeFrame = null;
      this.fitAndNotify();
      this.scheduleRefresh();
    });
  }

  private fitAndNotify(): void {
    if (!this.syncPTYResize) {
      this.applyPtyDimensions();
      return;
    }
    const terminal = this.term;
    const fitAddon = this.fitAddon;
    if (!terminal || !fitAddon) return;

    try {
      fitAddon.fit();
    } catch {
      return;
    }

    const { cols, rows } = terminal;
    if (cols > 0 && rows > 0 && (cols !== this.lastSentCols || rows !== this.lastSentRows)) {
      this.lastSentCols = cols;
      this.lastSentRows = rows;
      this.options.onResize(cols, rows);
    }
  }

  private applyPtyDimensions(): void {
    const terminal = this.term;
    if (!terminal) return;
    const { ptyCols: cols, ptyRows: rows } = this;
    if (cols > 0 && rows > 0 && (cols !== terminal.cols || rows !== terminal.rows)) {
      terminal.resize(cols, rows);
      this.scheduleRefresh();
    }
  }

  private scheduleRefresh(): void {
    if (this.refreshFrame !== null) return;
    this.refreshFrame = requestAnimationFrame(() => {
      this.refreshFrame = null;
      const terminal = this.term;
      if (!terminal) return;
      terminal.clearTextureAtlas();
      terminal.refresh(0, Math.max(0, terminal.rows - 1));
    });
  }

  private refreshAndNudgeAltBuffer(): void {
    const terminal = this.term;
    if (!terminal) return;
    this.scheduleRefresh();

    if (terminal.buffer.active.type !== 'alternate') return;
    const { cols, rows } = terminal;
    if (cols <= 1 || rows <= 1) return;

    terminal.resize(cols, rows - 1);
    this.options.onResize(cols, rows - 1);
    requestAnimationFrame(() => {
      if (this.disposed || this.term !== terminal) return;
      terminal.resize(cols, rows);
      this.options.onResize(cols, rows);
    });
  }
}
