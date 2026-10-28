// @vitest-environment jsdom
// Settings > General: "Bring my old chats". The status line counts the old chats in search, Remove asks first and
// calls the clear command, and Add or update opens the Bring panel (through the request store).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as chatImport from '../../../lib/chatImport';
import type { ImportState, ImportStatus } from '../../../lib/chatImport';
import { makeEnv, type TestEnv } from '../test-utils';
import { BringChatsSettings, BRING_CHATS_REMOVE_BODY, bringChatsStatusText } from './BringChatsSettings';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

function stateWith(chats: number): ImportState {
  return {
    asked: true,
    sinceDays: 365,
    indexEnabled: true,
    agents: [],
    progress: chatImport.idleProgress(),
    imported: { chats, items: chats * 10 },
  };
}

function spies(chats: number) {
  return {
    state: vi.spyOn(chatImport, 'chatImportState').mockResolvedValue(stateWith(chats)),
    clear: vi.spyOn(chatImport, 'chatImportClear').mockResolvedValue({ enabled: true, items: 0, bytes: 0, importedChats: 0, importedItems: 0 } as ImportStatus),
    request: vi.spyOn(chatImport, 'requestBringChats').mockImplementation(() => undefined),
  };
}

async function render(env: TestEnv) {
  await act(async () => {
    root.render(<BringChatsSettings env={env} />);
  });
  await act(async () => {
    await Promise.resolve();
  });
}

const text = () => host.textContent ?? '';
const buttonNamed = (name: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === name);

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

describe('bringChatsStatusText', () => {
  it('says Not added for none, and counts the chats otherwise', () => {
    expect(bringChatsStatusText(0)).toBe('Not added');
    expect(bringChatsStatusText(1)).toBe('1 chat');
    expect(bringChatsStatusText(538)).toBe('538 chats');
  });
});

describe('BringChatsSettings', () => {
  it('shows Not added and disables Remove when no old chats were added', async () => {
    spies(0);
    await render(makeEnv());
    expect(text()).toContain('Bring my old chats');
    expect(text()).toContain('Not added');
    expect(buttonNamed('Remove imported chats')?.disabled).toBe(true);
  });

  it('shows the count of old chats that are in search', async () => {
    spies(538);
    await render(makeEnv());
    expect(text()).toContain('538 chats');
    expect(text()).not.toContain('Not added');
    expect(buttonNamed('Remove imported chats')?.disabled).toBe(false);
  });

  it('asks before removing, and calls the clear command only after a yes', async () => {
    const s = spies(538);
    const env = makeEnv();
    // makeEnv sets its own confirm after the overrides, so answer "no" on the mock itself.
    env.confirm.mockResolvedValue(false);
    await render(env);
    await act(async () => buttonNamed('Remove imported chats')!.click());
    expect(env.confirm).toHaveBeenCalledWith(expect.objectContaining({ body: BRING_CHATS_REMOVE_BODY, confirmLabel: 'Remove' }));
    expect(s.clear).not.toHaveBeenCalled();
  });

  it('removes after the person confirms, then shows Not added', async () => {
    const s = spies(538);
    const env = makeEnv();
    await render(env);
    await act(async () => buttonNamed('Remove imported chats')!.click());
    await act(async () => {
      await Promise.resolve();
    });
    expect(s.clear).toHaveBeenCalledTimes(1);
    expect(env.markSaved).toHaveBeenCalled();
    expect(text()).toContain('Not added');
  });

  it('Add or update opens the Bring panel through the request store and reads no folder', async () => {
    const s = spies(0);
    await render(makeEnv());
    await act(async () => buttonNamed('Add or update')!.click());
    expect(s.request).toHaveBeenCalledTimes(1);
    expect(s.clear).not.toHaveBeenCalled();
  });
});
