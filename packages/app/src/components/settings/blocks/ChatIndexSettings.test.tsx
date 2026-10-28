// @vitest-environment jsdom
// Settings > General: the search index of your chats. The rows show what the index holds, the switch saves it, and
// the clear button asks first.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as chatIndex from '../../../lib/chatIndex';
import type { ChatIndexStatus } from '../../../lib/chatIndex';
import { makeEnv, type TestEnv } from '../test-utils';
import { ChatIndexSettings, CHAT_INDEX_CLEAR_BODY } from './ChatIndexSettings';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const SIZED: ChatIndexStatus = { enabled: true, items: 4312, bytes: 2_202_010 };

function spies(start: ChatIndexStatus = SIZED) {
  return {
    status: vi.spyOn(chatIndex, 'chatIndexStatus').mockResolvedValue(start),
    set: vi.spyOn(chatIndex, 'chatIndexSetEnabled').mockImplementation(async (enabled) => ({ ...start, enabled })),
    clear: vi.spyOn(chatIndex, 'chatIndexClear').mockResolvedValue({ enabled: true, items: 0, bytes: 0 }),
  };
}

async function render(env: TestEnv) {
  await act(async () => {
    root.render(<ChatIndexSettings env={env} />);
  });
  await act(async () => {
    await Promise.resolve();
  });
}

const toggle = () => host.querySelector<HTMLButtonElement>('button[role="switch"]');
const clearButton = () => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.startsWith('Clear'));
const text = () => host.textContent ?? '';

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe('ChatIndexSettings', () => {
  it('shows the size and the number of items, and that the index is on', async () => {
    spies();
    await render(makeEnv());
    expect(text()).toContain('2.1 MB · 4,312 items');
    expect(toggle()?.getAttribute('aria-checked')).toBe('true');
    expect(text()).toContain('Keep a search index of my chats');
  });

  it('turning it off saves the switch in Rust and shows it off', async () => {
    const s = spies();
    await render(makeEnv());
    await act(async () => {
      toggle()?.click();
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(s.set).toHaveBeenCalledWith(false);
    expect(toggle()?.getAttribute('aria-checked')).toBe('false');
  });

  it('asks before clearing, and clears only when the person says yes', async () => {
    const s = spies();
    const env = makeEnv();
    env.confirm.mockResolvedValue(false);
    await render(env);
    await act(async () => {
      clearButton()?.click();
    });
    expect(env.confirm).toHaveBeenCalledWith(expect.objectContaining({ body: CHAT_INDEX_CLEAR_BODY }));
    expect(CHAT_INDEX_CLEAR_BODY).toBe('This removes the saved search text. Chats themselves are not touched.');
    expect(s.clear).not.toHaveBeenCalled();
  });

  it('clears the index after the confirm and shows what is left', async () => {
    const s = spies();
    const env = makeEnv();
    env.confirm.mockResolvedValue(true);
    await render(env);
    await act(async () => {
      clearButton()?.click();
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(s.clear).toHaveBeenCalledTimes(1);
    expect(text()).toContain('0 B · 0 items');
  });

  it('shows a plain error line when the change fails', async () => {
    const s = spies();
    s.set.mockRejectedValueOnce(new Error('could not save the setting'));
    const env = makeEnv();
    await render(env);
    await act(async () => {
      toggle()?.click();
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(env.setError).toHaveBeenCalledWith('could not save the setting');
  });
});
