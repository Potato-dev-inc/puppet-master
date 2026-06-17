import type { TerminalTheme } from './canvas-terminal';

function cssRgb(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value ? `rgb(${value})` : fallback;
}

/** Build a terminal theme from Puppet Master CSS variables. */
export function terminalThemeFromCss(): TerminalTheme {
  const background = cssRgb('--pm-bg', '#0a0a0a');
  const foreground = cssRgb('--pm-text', '#e8e8e8');
  const cursor = cssRgb('--pm-accent', '#f5f5f5');

  return {
    background,
    foreground,
    cursor,
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
}

export const TERMINAL_FONT_FAMILY = '"Cascadia Code", Consolas, "Courier New", monospace';
export const TERMINAL_FONT_SIZE = 12;
export const TERMINAL_SCROLLBACK = 5000;
