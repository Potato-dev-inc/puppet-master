// @vitest-environment jsdom
// "Agent is asking": the card for a waiting pane that no other card answers. It reads the screen, offers the rows as
// buttons that send the right keys, takes a typed answer, and is gone when the pane stops waiting or the screen is empty.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const readSnapshot = vi.fn<(paneId: string) => Promise<string>>();
const writeInput = vi.fn<(paneId: string, text: string, appendNewline?: boolean) => Promise<void>>(async () => undefined);
vi.mock('../../lib/tauri', () => ({
  tauri: {
    readSnapshot: (paneId: string) => readSnapshot(paneId),
    writeInput: (paneId: string, text: string, appendNewline?: boolean) => writeInput(paneId, text, appendNewline),
  },
}));

import { ScreenQuestionCard } from './ScreenQuestionCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const LOGIN = [
  ' Login',
  ' Select login method:',
  '',
  ' ❯ 1. Claude account with subscription · Pro, Max, Team, or Enterprise',
  '   2. Anthropic Console account · API usage billing',
  '   3. 3rd-party platform · Amazon Bedrock, Microsoft Foundry, Google Vertex AI',
  '',
  ' Esc to cancel',
].join('\n');

let host: HTMLDivElement;
let root: Root;

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
const buttons = () => Array.from(host.querySelectorAll('button'));
const button = (label: string) => buttons().find((b) => b.textContent === label)!;

async function show(props: Partial<Parameters<typeof ScreenQuestionCard>[0]> = {}) {
  await act(async () => root.render(<ScreenQuestionCard paneId="p1" waiting {...props} />));
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

describe('ScreenQuestionCard', () => {
  it('shows the question and one button per option of a menu', async () => {
    readSnapshot.mockResolvedValue(LOGIN);
    await show();
    expect(host.querySelector('[data-screen-question]')).not.toBeNull();
    expect(host.textContent).toContain('Agent is asking');
    expect(host.textContent).toContain('Select login method:');
    expect(buttons().map((b) => b.textContent)).toEqual(
      expect.arrayContaining(['Claude account with subscription', 'Anthropic Console account', '3rd-party platform']),
    );
  });

  it('moves the pointer with arrow keys, then presses Enter on its own', async () => {
    readSnapshot.mockResolvedValue(LOGIN);
    await show();
    await act(async () => button('3rd-party platform').click());
    expect(writeInput.mock.calls).toEqual([
      ['p1', '\u001b[B\u001b[B', false],
      ['p1', '', true],
    ]);
  });

  it('answers the row the pointer is on with Enter only', async () => {
    readSnapshot.mockResolvedValue(LOGIN);
    await show();
    await act(async () => button('Claude account with subscription').click());
    expect(writeInput.mock.calls).toEqual([['p1', '', true]]);
  });

  it('shows Yes and No for a (y/n) question and types the letter with Enter', async () => {
    readSnapshot.mockResolvedValue('Overwrite config.json? (y/n)');
    await show();
    await act(async () => button('No').click());
    expect(writeInput.mock.calls).toEqual([
      ['p1', 'n', false],
      ['p1', '', true],
    ]);
    writeInput.mockClear();
    await act(async () => button('Yes').click());
    expect(writeInput.mock.calls[0]).toEqual(['p1', 'y', false]);
  });

  it('sends a typed answer with Enter', async () => {
    readSnapshot.mockResolvedValue('What should the project be called?');
    await show();
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Type an answer"]')!;
    expect(button('Send').disabled).toBe(true);
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
      set.call(input, 'shop');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => button('Send').click());
    expect(writeInput.mock.calls).toEqual([['p1', 'shop', true]]);
    expect(input.value).toBe('');
  });

  it('has the key buttons Up, Down, Enter, Esc and Tab', async () => {
    readSnapshot.mockResolvedValue('Press a key');
    await show();
    for (const [label, keys] of [
      ['Up', '\u001b[A'],
      ['Down', '\u001b[B'],
      ['Esc', '\u001b'],
      ['Tab', '\t'],
    ] as const) {
      writeInput.mockClear();
      await act(async () => button(label).click());
      expect(writeInput.mock.calls).toEqual([['p1', keys, false]]);
    }
    writeInput.mockClear();
    await act(async () => button('Enter').click());
    expect(writeInput.mock.calls).toEqual([['p1', '', true]]);
  });

  it('keeps "Open Terminal" as a quiet way out, only when it can', async () => {
    readSnapshot.mockResolvedValue(LOGIN);
    const open = vi.fn();
    await show({ onOpenTerminal: open });
    await act(async () => button('Open Terminal').click());
    expect(open).toHaveBeenCalledTimes(1);
    await show({ onOpenTerminal: undefined });
    expect(buttons().find((b) => b.textContent === 'Open Terminal')).toBeUndefined();
  });

  it('is hidden, and reads nothing, when the pane is not waiting', async () => {
    readSnapshot.mockResolvedValue(LOGIN);
    await show({ waiting: false });
    expect(host.querySelector('[data-screen-question]')).toBeNull();
    expect(readSnapshot).not.toHaveBeenCalled();
  });

  it('goes away when the pane stops waiting', async () => {
    readSnapshot.mockResolvedValue(LOGIN);
    await show();
    expect(host.querySelector('[data-screen-question]')).not.toBeNull();
    await show({ waiting: false });
    expect(host.querySelector('[data-screen-question]')).toBeNull();
  });

  it('is hidden when the screen is empty and there is nothing else to say, and falls back to the known text', async () => {
    readSnapshot.mockResolvedValue('');
    await show();
    expect(host.querySelector('[data-screen-question]')).toBeNull();
    await show({ fallbackText: 'Claude Code needs you to sign in' });
    expect(host.textContent).toContain('Claude Code needs you to sign in');
  });
});
