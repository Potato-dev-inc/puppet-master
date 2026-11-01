import type { TerminalTheme } from './canvas-terminal';

function cssRgb(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value ? `rgb(${value})` : fallback;
}

/** Monochrome ANSI palette aligned with Puppet Master CSS tokens. */
export function terminalThemeFromCss(): TerminalTheme {
  const background = cssRgb('--pm-bg', '#1b1a19');
  const foreground = cssRgb('--pm-text', '#f4f2ef');
  const cursor = cssRgb('--pm-accent', '#f29a3c');
  const muted = cssRgb('--pm-muted', '#969696');
  const raised = cssRgb('--pm-raised', '#292725');
  const warn = cssRgb('--pm-warn', '#b4b4b4');
  const err = cssRgb('--pm-err', '#ffffff');

  return {
    background,
    foreground,
    cursor,
    cursorAccent: background,
    selectionBackground: raised,
    selectionForeground: foreground,
    selectionInactiveBackground: raised,
    black: background,
    red: err,
    green: foreground,
    yellow: warn,
    blue: muted,
    magenta: foreground,
    cyan: muted,
    white: foreground,
    brightBlack: muted,
    brightRed: err,
    brightGreen: foreground,
    brightYellow: warn,
    brightBlue: foreground,
    brightMagenta: foreground,
    brightCyan: foreground,
    brightWhite: foreground,
  };
}

export const TERMINAL_FONT_FAMILY = '"Cascadia Code", Consolas, "Courier New", monospace';
export const TERMINAL_FONT_SIZE = 12;
export const TERMINAL_SCROLLBACK = 5000;
