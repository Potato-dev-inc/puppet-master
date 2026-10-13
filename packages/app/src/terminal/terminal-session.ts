import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { InputBatcher } from './input-batcher';
import { MirrorEchoFilter } from './mirror-echo-filter';
import { applyMirrorLocalEcho, isTerminalReport } from './mirror-local-echo';
import {
  configureXtermTextareaForMobileMirror,
  isMobileInputDevice,
  MobileInputGuard,
  type MobileInputDelivery,
} from './mobile-input-guard';
import { configureDesktopXtermTextarea, isTerminalPasteShortcut } from './xterm-textarea';
import { TouchScrollController } from './touch-scroll';
import { ownerResizeDecision } from './owner-follow';
import {
  TERMINAL_FONT_FAMILY,
  TERMINAL_FONT_SIZE,
  TERMINAL_SCROLLBACK,
  terminalThemeFromCss,
} from './theme';
import {
  createTerminalScaleMount,
  TerminalScaleController,
} from './scaled-viewport';
import type { PaneDataSubscriber, TerminalRenderMode, TerminalSessionOptions } from './types';
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
  private muteOnData = false;
  private readonly mirrorEchoFilter = new MirrorEchoFilter();
  private focusCleanup: (() => void) | null = null;
  private pasteCleanup: (() => void) | null = null;
  private clipboardShortcutCleanup: (() => void) | null = null;
  private openFrame: number | null = null;
  private resizeFrame: number | null = null;
  private refreshFrame: number | null = null;
  private lastSentCols = 0;
  private lastSentRows = 0;
  private ptyCols: number;
  private ptyRows: number;
  /**
   * The pane's resizes arrive in order with its output (`applyStreamResize`): from then on the
   * React prop path (`setPtyDimensions`) must not resize ahead of bytes still being parsed.
   */
  private streamSized = false;
  /** Owner window: the PTY size while a phone holds it (null when it is the size this window asked for). */
  private followPty: { cols: number; rows: number } | null = null;
  /** Owner window: a PTY resize has arrived in stream order, so a size seen at mount is stale. */
  private ownerStreamResizeSeen = false;
  /** Owner window: the size a phone holds, from the registry; applied once the grid is open. */
  private heldPty: { cols: number; rows: number } | null = null;
  private readonly syncPTYResize: boolean;
  private readonly mirrorPTY: boolean;
  private readonly renderMode: TerminalRenderMode;
  private readonly mobileInputDisabled: boolean;
  private generation = 0;
  private themeObserver: MutationObserver | null = null;
  private scaleController: TerminalScaleController | null = null;
  private scaleViewport: HTMLElement | null = null;
  private mountContainer: HTMLElement | null = null;
  private touchScroll: TouchScrollController | null = null;
  private disposed = false;

  constructor(private readonly options: TerminalSessionOptions) {
    this.syncPTYResize = options.syncPTYResize ?? true;
    this.mirrorPTY = !this.syncPTYResize;
    this.renderMode = options.renderMode ?? (this.syncPTYResize ? 'owner' : 'mirror-same-grid');
    this.mobileInputDisabled = options.disableMobileInput === true;
    this.ptyCols = options.ptyCols ?? 80;
    this.ptyRows = options.ptyRows ?? 24;
  }

  /**
   * Force a refit/repaint at the current host size. Used as a safety net when
   * the host layout changes in a way the ResizeObserver might miss — e.g. a pane
   * returning from a detached window. The grid also plays a visible
   * shrink-and-restore animation on the cell (see `pm-pane-settle`), which drives
   * the ResizeObserver every frame; this call just guarantees a final clean fit.
   */
  nudgeReflow(): boolean {
    if (!this.term) return false;
    if (this.syncPTYResize) {
      this.fitAndNotify();
      this.refreshAndNudgeAltBuffer();
    } else {
      this.applyMirrorDimensions();
    }
    return true;
  }

  /**
   * The grid (cols x rows) that fits this terminal's on-screen area at the current font: what a
   * phone asks the PC to size the pane to (`POST /panes/{id}/viewport`) so the agent redraws for
   * the phone. Null until xterm has measured its cells.
   */
  measureFitGrid(): { cols: number; rows: number } | null {
    const metrics = this.gridMetrics();
    const host = this.scaleViewport ?? this.mountContainer;
    if (!metrics || !host) return null;
    const cols = Math.floor((host.clientWidth - metrics.padX) / metrics.cellWidth);
    const rows = Math.floor((host.clientHeight - metrics.padY) / metrics.cellHeight);
    return cols > 0 && rows > 0 ? { cols, rows } : null;
  }

  /**
   * Cell size and the padding xterm draws around the grid. Arithmetic only: the rendered boxes
   * (`.xterm`, `.xterm-screen`, the scale stage) keep an old size after an in-place resize, and
   * sizing from them shrank or scaled the grid wrongly.
   */
  private gridMetrics(): { cellWidth: number; cellHeight: number; padX: number; padY: number } | null {
    const terminal = this.term;
    if (!terminal?.element) return null;
    // Same private measurement FitAddon.proposeDimensions uses.
    const core = (terminal as unknown as {
      _core?: { _renderService?: { dimensions?: { css?: { cell?: { width: number; height: number } } } } };
    })._core;
    const cell = core?._renderService?.dimensions?.css?.cell;
    if (!cell || cell.width <= 0 || cell.height <= 0) return null;
    const style = getComputedStyle(terminal.element);
    const px = (value: string) => Number.parseFloat(value) || 0;
    return {
      cellWidth: cell.width,
      cellHeight: cell.height,
      padX: px(style.paddingLeft) + px(style.paddingRight),
      padY: px(style.paddingTop) + px(style.paddingBottom),
    };
  }

  /** Pixel size of the current grid (cells plus padding), for the scale controller. */
  private gridContentSize(): { width: number; height: number } | null {
    const metrics = this.gridMetrics();
    const terminal = this.term;
    if (!metrics || !terminal) return null;
    return {
      width: terminal.cols * metrics.cellWidth + metrics.padX,
      height: terminal.rows * metrics.cellHeight + metrics.padY,
    };
  }

  setPtyDimensions(cols: number, rows: number): void {
    if (this.streamSized) return;
    this.resizeToPty(cols, rows);
  }

  /**
   * A pane resize from the PC, in order with its output: applied once every byte before it has
   * been parsed, so output drawn for the old grid is never laid out on the new one (and output
   * for the new grid never on the old one).
   */
  applyStreamResize(cols: number, rows: number): void {
    const terminal = this.term;
    if (!terminal || cols <= 0 || rows <= 0) return;
    if (this.syncPTYResize) {
      this.ownerStreamResizeSeen = true;
      terminal.write(new Uint8Array(0), () => {
        if (this.term === terminal && !this.disposed) this.applyOwnerFollow(cols, rows);
      });
      return;
    }
    this.streamSized = true;
    terminal.write(new Uint8Array(0), () => {
      if (this.term === terminal && !this.disposed) this.resizeToPty(cols, rows);
    });
  }

  /**
   * Owner window, in stream order: follow the PTY while a phone holds it, and stop following when
   * the PTY is back at the size this window asked for.
   */
  private applyOwnerFollow(cols: number, rows: number): void {
    const terminal = this.term;
    if (!terminal) return;
    const decision = ownerResizeDecision(
      { cols, rows },
      { cols: this.lastSentCols, rows: this.lastSentRows },
      { cols: terminal.cols, rows: terminal.rows },
    );
    this.followPty = decision.follow;
    if (decision.resizeTo) terminal.resize(decision.resizeTo.cols, decision.resizeTo.rows);
    this.scheduleRefresh();
  }

  /**
   * Owner window opened while a phone already holds the pane: no resize event comes for that, so
   * follow the held size. Ignored once an ordered resize has arrived.
   */
  followHeldPty(cols: number, rows: number): void {
    if (!this.syncPTYResize || cols <= 0 || rows <= 0) return;
    this.heldPty = { cols, rows };
    this.applyHeldPty();
  }

  private applyHeldPty(): void {
    const terminal = this.term;
    const held = this.heldPty;
    if (!terminal || !held || this.ownerStreamResizeSeen) return;
    terminal.write(new Uint8Array(0), () => {
      if (this.term !== terminal || this.disposed || this.ownerStreamResizeSeen) return;
      this.applyOwnerFollow(held.cols, held.rows);
    });
  }

  private resizeToPty(cols: number, rows: number): void {
    if (cols <= 0 || rows <= 0) return;
    if (this.mirrorPTY) {
      this.ptyCols = cols;
      this.ptyRows = rows;
      this.applyMirrorDimensions();
      return;
    }
    this.ptyCols = cols;
    this.ptyRows = rows;
    this.applyPtyDimensions();
  }

  private isMobileMirror(): boolean {
    return !this.mobileInputDisabled && this.mirrorPTY && isMobileInputDevice();
  }

  /** Swipe-to-scroll for the phone mirror. Touch points are mapped to cells through the scale. */
  private createTouchScroll(container: HTMLElement, terminal: Terminal): TouchScrollController {
    // Screen pixels per layout pixel (the scale-to-fit transform), so drags map to rows on screen.
    const screenScale = (): number => {
      const el = terminal.element;
      if (!el || el.offsetWidth <= 0) return 1;
      return el.getBoundingClientRect().width / el.offsetWidth || 1;
    };
    return new TouchScrollController(container, terminal, {
      sendInput: (data) => this.options.onInput(data, false),
      cellHeight: () => (this.gridMetrics()?.cellHeight ?? 0) * screenScale(),
      cellFromPoint: (clientX, clientY) => {
        const metrics = this.gridMetrics();
        const el = terminal.element;
        if (!metrics || !el) return { col: 1, row: 1 };
        const rect = el.getBoundingClientRect();
        const scale = screenScale();
        const x = (clientX - rect.left) / scale - metrics.padX / 2;
        const y = (clientY - rect.top) / scale - metrics.padY / 2;
        const col = Math.min(Math.max(Math.floor(x / metrics.cellWidth) + 1, 1), Math.max(terminal.cols, 1));
        const row = Math.min(Math.max(Math.floor(y / metrics.cellHeight) + 1, 1), Math.max(terminal.rows, 1));
        return { col, row };
      },
      onScrollStart: () => this.mobileInputGuard?.abandonGesture(),
      panViewport: (dy) => this.scaleController?.panBy(dy) ?? dy,
      isPanned: () => this.scaleController?.isPanned() ?? false,
      resetPan: () => this.scaleController?.resetPan(),
    });
  }

  private shouldScaleMirrorToContainer(): boolean {
    return this.isMobileMirror() && this.renderMode === 'mirror-same-grid';
  }

  mount(
    container: HTMLElement,
    subscribePaneData: PaneDataSubscriber,
  ): void {
    const gen = ++this.generation;
    this.disposed = false;
    this.mountContainer = container;

    this.openFrame = requestAnimationFrame(() => {
      this.openFrame = null;
      if (this.disposed || gen !== this.generation || !container.isConnected) {
        return;
      }

      const mobileMirror = this.isMobileMirror();
      const terminal = new Terminal({
        theme: terminalThemeFromCss(),
        fontFamily: TERMINAL_FONT_FAMILY,
        fontSize: TERMINAL_FONT_SIZE,
        scrollback: TERMINAL_SCROLLBACK,
        cursorBlink: true,
        convertEol: false,
        allowProposedApi: true,
        allowTransparency: false,
        scrollOnUserInput: !mobileMirror,
        disableStdin: false,
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

      let mountTarget = container;
      if (this.shouldScaleMirrorToContainer()) {
        const { viewport, stage } = createTerminalScaleMount(container);
        this.scaleViewport = viewport;
        mountTarget = stage;
      }

      terminal.open(mountTarget);

      this.term = terminal;
      this.fitAddon = fitAddon;
      if (this.scaleViewport) {
        this.scaleController = new TerminalScaleController(
          this.scaleViewport,
          mountTarget,
          this.options.sizesPane
            ? {
                keyboardPushUp: false,
                measureContent: () => this.gridContentSize(),
                noScale: true,
              }
            : {},
        );
      }
      this.inputBatcher = new InputBatcher((text) => {
        this.options.onInput(text, false);
      }, 4);

      terminal.onData((data) => {
        if (this.muteOnData) return;
        if (!mobileMirror) {
          this.inputBatcher?.push(data);
          this.scrollToCursor();
          this.scheduleRefresh();
          return;
        }

        // xterm answering the agent's own queries (cursor position, device attributes): the
        // desktop window that owns the pane answers those; a mirror must not type them too.
        if (isTerminalReport(data)) return;

        // Hardware keyboard — local echo immediately; PTY echo is deduped on ingest.
        this.applyLocalEcho(terminal, data);
        this.inputBatcher?.push(data);
        this.scrollToCursor();
        this.scheduleRefresh();
      });

      this.unlistenData = subscribePaneData(this.options.paneId, (data) => {
        if (
          this.mirrorPTY &&
          !this.options.sizesPane &&
          isMobileInputDevice() &&
          this.mirrorEchoFilter.shouldSkipInbound(data)
        ) {
          return;
        }
        terminal.write(data, () => {
          this.scrollToCursor();
          this.scheduleRefresh();
        });
      }, (cols, rows) => this.applyStreamResize(cols, rows));
      this.installFileLinkProvider(terminal);
      this.installTextareaGuards(container, terminal, this.inputBatcher);
      if (mobileMirror) {
        this.touchScroll = this.createTouchScroll(container, terminal);
      }
      this.installResizeObservers(container);
      this.installFocusHandlers(container);
      this.installThemeObserver(terminal);
      if (this.syncPTYResize) {
        this.fitAndNotify();
        // A fresh owner mount (e.g. a pane reattaching from a detached window
        // back into the grid) inherits a PTY still sized to its previous home.
        // Fitting alone often leaves alt-buffer TUIs painted at the old grid, so
        // nudge the layout once the new size has settled to force a clean reflow.
        requestAnimationFrame(() => {
          if (this.disposed || gen !== this.generation) return;
          this.fitAndNotify();
        });
        this.applyHeldPty();
      } else {
        this.applyMirrorDimensions();
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

    this.touchScroll?.dispose();
    this.touchScroll = null;

    this.scaleController?.dispose();
    this.scaleController = null;
    this.scaleViewport?.remove();
    this.scaleViewport = null;
    this.mountContainer?.classList.remove('terminal-host--mirror-scale');
    this.mountContainer = null;

    this.themeObserver?.disconnect();
    this.themeObserver = null;

    this.focusCleanup?.();
    this.focusCleanup = null;

    this.pasteCleanup?.();
    this.pasteCleanup = null;

    this.clipboardShortcutCleanup?.();
    this.clipboardShortcutCleanup = null;

    this.inputBatcher?.dispose();
    this.inputBatcher = null;

    this.fitAddon = null;
    this.term?.dispose();
    this.term = null;
  }

  private installResizeObservers(container: HTMLElement): void {
    const observed = this.scaleViewport ?? container;
    this.resizeObserver = new ResizeObserver(() => {
      this.scheduleFit();
    });
    this.resizeObserver.observe(observed);

    this.intersectionObserver = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          window.setTimeout(() => {
            if (this.syncPTYResize) {
              this.fitAndNotify();
              this.refreshAndNudgeAltBuffer();
            } else {
              this.applyMirrorDimensions();
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

  private installPasteFallback(
    container: HTMLElement,
    terminal: Terminal,
    mobileMirror: boolean,
  ): void {
    if (mobileMirror) return;

    const onPaste = (event: ClipboardEvent) => {
      const text = event.clipboardData?.getData('text/plain');
      if (!text) return;

      event.preventDefault();
      event.stopPropagation();
      terminal.focus();
      this.inputBatcher?.flushNow();
      terminal.paste(text);
      this.scrollToCursor();
      this.scheduleRefresh();
    };

    container.addEventListener('paste', onPaste, true);
    this.pasteCleanup = () => {
      container.removeEventListener('paste', onPaste, true);
    };
  }

  private installClipboardShortcutFallback(
    container: HTMLElement,
    terminal: Terminal,
    mobileMirror: boolean,
  ): void {
    if (mobileMirror) return;

    const pasteFromClipboard = async (event: KeyboardEvent) => {
      if (!isTerminalPasteShortcut(event)) return;
      const clipboard = navigator.clipboard;
      if (!clipboard?.readText) return;

      event.preventDefault();
      event.stopPropagation();

      let text = '';
      try {
        text = await clipboard.readText();
      } catch {
        return;
      }
      if (!text || this.disposed) return;

      terminal.focus();
      this.inputBatcher?.flushNow();
      terminal.paste(text);
      this.scrollToCursor();
      this.scheduleRefresh();
    };

    container.addEventListener('keydown', pasteFromClipboard, true);
    this.clipboardShortcutCleanup = () => {
      container.removeEventListener('keydown', pasteFromClipboard, true);
    };
  }

  private installFocusHandlers(container: HTMLElement): void {
    if (this.mobileInputGuard) {
      return;
    }

    const focusTerminal = () => {
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
    const mobileMirror = this.isMobileMirror();

    const fixImePosition = () => {
      if (!textarea) return;
      if (mobileMirror) {
        configureXtermTextareaForMobileMirror(textarea);
        return;
      }
      configureDesktopXtermTextarea(container, textarea);
    };

    if (textarea) {
      this.imeObserver = new MutationObserver(fixImePosition);
      this.imeObserver.observe(textarea, { attributes: true, attributeFilter: ['style'] });
    }
    fixImePosition();
    this.installPasteFallback(container, terminal, mobileMirror);
    this.installClipboardShortcutFallback(container, terminal, mobileMirror);

    if (mobileMirror) {
      const emitMobileInput = (text: string, delivery: MobileInputDelivery) => {
        if (!text) return;
        if (delivery === 'immediate') {
          inputBatcher.flushNow();
        }

        // Local echo only erases chars we echoed; filter tracks outbound for dedupe.
        this.applyLocalEcho(terminal, text);

        if (delivery === 'immediate') {
          this.options.onInput(text, false);
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
        bufferDelayMs: this.options.mobileInputDelayMs,
        inputVisible: this.options.mobileInputVisible,
        // Phone-sized panes live in a shell that already ends at the keyboard (PwaApp).
        reserveKeyboardSpace: !this.options.sizesPane,
      });
      requestAnimationFrame(() => {
        if (this.disposed) return;
        this.applyMirrorDimensions();
        this.scrollToCursor();
      });
    }
  }

  /**
   * Draw typed text before the PTY echoes it (mirrors over a slow link feel instant). Muted:
   * echoing an escape key re-enters xterm's input, which would fire onData again forever. Not for
   * phone-sized panes: a full-screen agent redraws its input box its own way, so guessed
   * characters land in the wrong cells and survive; there the agent's echo is the only truth.
   */
  private applyLocalEcho(terminal: Terminal, text: string): void {
    if (this.options.sizesPane) return;
    this.muteOnData = true;
    try {
      applyMirrorLocalEcho(terminal, text, this.mirrorEchoFilter);
    } finally {
      this.muteOnData = false;
    }
  }

  private scrollToCursor(): void {
    const terminal = this.term;
    if (!terminal) return;

    terminal.scrollToBottom();

    if (!this.isMobileMirror()) return;

    const pinMobileScroll = (): void => {
      const xtermEl = terminal.element;
      if (!xtermEl) return;
      xtermEl.scrollTop = xtermEl.scrollHeight;
      const viewport = xtermEl.querySelector('.xterm-viewport');
      if (viewport instanceof HTMLElement) {
        viewport.scrollTop = viewport.scrollHeight;
      }
    };

    requestAnimationFrame(() => {
      pinMobileScroll();
      requestAnimationFrame(pinMobileScroll);
    });
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
      this.applyMirrorDimensions();
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
      this.applyMirrorDimensions();
      return;
    }
    const terminal = this.term;
    const fitAddon = this.fitAddon;
    if (!terminal || !fitAddon) return;

    if (this.followPty !== null) {
      // A phone holds the PTY: the grid stays at the PTY size; the PC learns this window's own size.
      let dims: { cols: number; rows: number } | undefined;
      try {
        dims = fitAddon.proposeDimensions();
      } catch {
        return;
      }
      if (dims && dims.cols > 0 && dims.rows > 0 && (dims.cols !== this.lastSentCols || dims.rows !== this.lastSentRows)) {
        this.lastSentCols = dims.cols;
        this.lastSentRows = dims.rows;
        this.options.onResize(dims.cols, dims.rows);
      }
      return;
    }

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
      terminal.scrollToBottom();
      this.scheduleRefresh();
    }
  }

  /** Mirror viewers on mobile: fit xterm to the local viewport without resizing the PTY. */
  private fitViewportOnly(): void {
    const terminal = this.term;
    const fitAddon = this.fitAddon;
    if (!terminal || !fitAddon) return;
    try {
      fitAddon.fit();
    } catch {
      return;
    }
    terminal.scrollToBottom();
    this.scheduleRefresh();
  }

  private applyMirrorDimensions(): void {
    if (this.renderMode === 'mirror-same-grid') {
      this.applyPtyDimensions();
      this.scaleController?.scheduleFit();
      return;
    }
    this.fitViewportOnly();
  }

  private scheduleRefresh(): void {
    if (this.refreshFrame !== null) return;
    this.refreshFrame = requestAnimationFrame(() => {
      this.refreshFrame = null;
      const terminal = this.term;
      if (!terminal) return;
      terminal.clearTextureAtlas();
      terminal.refresh(0, Math.max(0, terminal.rows - 1));
      this.scaleController?.scheduleFit();
    });
  }

  private refreshAndNudgeAltBuffer(): void {
    const terminal = this.term;
    // While following a phone-held PTY, the fake resize below would fight the lease.
    if (!terminal || this.followPty !== null) return;
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
