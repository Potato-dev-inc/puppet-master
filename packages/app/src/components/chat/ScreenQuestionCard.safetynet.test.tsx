// @vitest-environment jsdom
// The safety net of "Agent is asking": a pane that is only idle gets the card when its screen shows a menu with a
// cursor, or when a message the person wrote has been stuck for a few seconds. The idle input box of an agent never
// does, and a card is never added next to another card.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { OutboxMessage } from '../../lib/paneOutbox';

const readSnapshot = vi.fn<(paneId: string) => Promise<string>>();
const writeInput = vi.fn<(paneId: string, text: string, appendNewline?: boolean) => Promise<void>>(async () => undefined);
vi.mock('../../lib/tauri', () => ({
  tauri: {
    readSnapshot: (paneId: string) => readSnapshot(paneId),
    writeInput: (paneId: string, text: string, appendNewline?: boolean) => writeInput(paneId, text, appendNewline),
  },
}));

import { hasStuckMessage, ScreenQuestionCard, STUCK_MS } from './ScreenQuestionCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const fixture = (path: string) => readFileSync(resolve(__dirname, '../../../src-tauri/src', path), 'utf8');
const THEME = fixture('tui/claude/fixtures/first_run_theme_2_1_312.txt');
const CLAUDE_IDLE = fixture('tui/claude/fixtures/first_run_idle_box_2_1_312.txt');
const CODEX_IDLE = fixture('tui/codex/fixtures/standby_ready_live.txt');
const NUMBERED = ['Which package manager?', '  1) npm', '  2) pnpm', '  3) yarn', 'Enter a number:'].join('\n');

const message = (over: Partial<OutboxMessage> = {}): OutboxMessage => ({
  id: 'm1',
  text: 'hi',
  createdAt: Date.now() - (STUCK_MS + 2000),
  state: 'queued',
  waitingFor: 'Waiting for the agent. If it asks something, answer it first.',
  error: null,
  ...over,
});

let host: HTMLDivElement;
let root: Root;
const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
const buttons = () => Array.from(host.querySelectorAll('button'));
const card = () => host.querySelector('[data-screen-question]');

async function show(props: Partial<Parameters<typeof ScreenQuestionCard>[0]> = {}) {
  await act(async () => root.render(<ScreenQuestionCard paneId="p1" waiting={false} idle {...props} />));
  await flush();
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  readSnapshot.mockReset();
  writeInput.mockClear();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('ScreenQuestionCard as a safety net for an idle pane', () => {
  it('shows the theme picker of a pane that reads idle, as a card with the seven rows', async () => {
    readSnapshot.mockResolvedValue(THEME);
    await show();
    expect(card()).not.toBeNull();
    expect(host.textContent).toContain('Agent is asking');
    expect(host.textContent).toContain('Choose the text style that looks best with your terminal');
    expect(host.textContent).not.toContain('function greet');
    const labels = buttons().map((b) => b.textContent);
    expect(labels).toEqual(expect.arrayContaining(['Auto (match terminal)', 'Dark mode', 'Light mode', 'Light mode (ANSI colors only)']));
    expect(labels).not.toContain('✔ Dark mode');
  });

  it('answers a row of that card with arrow keys and then Enter', async () => {
    readSnapshot.mockResolvedValue(THEME);
    await show();
    await act(async () => buttons().find((b) => b.textContent === 'Light mode (colorblind-friendly)')!.click());
    expect(writeInput.mock.calls).toEqual([
      ['p1', '\u001b[B\u001b[B\u001b[B', false], // Dark mode is row 2, this is row 5
      ['p1', '', true],
    ]);
  });

  it.each([
    ['claude', CLAUDE_IDLE],
    ['codex', CODEX_IDLE],
  ])('shows nothing for the idle input box of %s', async (_agent, screen) => {
    readSnapshot.mockResolvedValue(screen);
    await show();
    expect(readSnapshot).toHaveBeenCalled();
    expect(card()).toBeNull();
  });

  it('shows nothing for a finished reply that ends in a numbered list with no cursor', async () => {
    readSnapshot.mockResolvedValue(NUMBERED);
    await show();
    expect(card()).toBeNull();
  });

  it('does not read, or show, anything for a pane that is working', async () => {
    readSnapshot.mockResolvedValue(THEME);
    await show({ idle: false, waiting: false });
    expect(readSnapshot).not.toHaveBeenCalled();
    expect(card()).toBeNull();
  });

  it('adds nothing next to a card that already answers the pane, and reads nothing', async () => {
    readSnapshot.mockResolvedValue(THEME);
    await show({ covered: true });
    expect(card()).toBeNull();
    expect(readSnapshot).not.toHaveBeenCalled();
    await show({ covered: true, waiting: true, idle: false });
    expect(card()).toBeNull();
    expect(readSnapshot).not.toHaveBeenCalled();
  });

  it('goes away when the pane starts working or the menu is answered', async () => {
    readSnapshot.mockResolvedValue(THEME);
    await show();
    expect(card()).not.toBeNull();
    await show({ idle: false });
    expect(card()).toBeNull();
    readSnapshot.mockResolvedValue(CLAUDE_IDLE);
    await show();
    expect(card()).toBeNull();
  });
});

describe('ScreenQuestionCard with a message stuck in the queue', () => {
  it('shows a menu with no cursor once a message waited more than a few seconds', async () => {
    readSnapshot.mockResolvedValue(NUMBERED);
    await show({ queued: [message()] });
    expect(card()).not.toBeNull();
    expect(buttons().map((b) => b.textContent)).toEqual(expect.arrayContaining(['npm', 'pnpm', 'yarn']));
    await act(async () => buttons().find((b) => b.textContent === 'pnpm')!.click());
    expect(writeInput.mock.calls[0]).toEqual(['p1', '2', false]);
  });

  it('waits for the few seconds before it shows that menu', async () => {
    readSnapshot.mockResolvedValue(NUMBERED);
    await show({ queued: [message({ createdAt: Date.now() - 500 })] });
    expect(card()).toBeNull();
    await show({ queued: [message({ state: 'sent' })] });
    expect(card()).toBeNull();
  });

  it('shows a yes / no question for a stuck message, but not plain text', async () => {
    readSnapshot.mockResolvedValue('Overwrite config.json? (y/n)');
    await show({ queued: [message()] });
    expect(buttons().map((b) => b.textContent)).toEqual(expect.arrayContaining(['Yes', 'No']));
    readSnapshot.mockResolvedValue('Setting up.\nWhat should the project be called?');
    await show({ queued: [message()], paneId: 'p2' });
    expect(card()).toBeNull();
  });

  it('does not take the idle input box for a question just because a message waits', async () => {
    for (const screen of [CLAUDE_IDLE, CODEX_IDLE]) {
      readSnapshot.mockResolvedValue(screen);
      await show({ queued: [message()] });
      expect(card()).toBeNull();
    }
  });

  it('knows a stuck message from a fresh one', () => {
    const now = 1_000_000;
    expect(hasStuckMessage([message({ createdAt: now - STUCK_MS })], now)).toBe(true);
    expect(hasStuckMessage([message({ createdAt: now - STUCK_MS + 1 })], now)).toBe(false);
    expect(hasStuckMessage([message({ createdAt: now - 60_000, state: 'failed' })], now)).toBe(false);
    expect(hasStuckMessage([], now)).toBe(false);
  });
});

describe('ScreenQuestionCard on a waiting pane', () => {
  it('shows the theme picker with its heading and rows, not the code preview', async () => {
    readSnapshot.mockResolvedValue(THEME);
    await show({ waiting: true, idle: false });
    const lines = host.querySelector('[data-role="screen-lines"]')!.textContent!;
    expect(lines).toContain('Choose the text style that looks best with your terminal');
    expect(lines).toContain('Dark mode (ANSI colors only)');
    expect(lines).not.toMatch(/function greet|Syntax theme|\.{4,}|╌/);
  });
});
