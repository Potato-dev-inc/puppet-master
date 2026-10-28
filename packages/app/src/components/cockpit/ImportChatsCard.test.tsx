// @vitest-environment jsdom
// "Bring my old chats" first-start card: fake api and settings, no agent folder touched.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ImportApi, ImportState } from '../../lib/chatImport';
import { ImportChatsCard, IMPORT_CARD_BODY, IMPORT_CARD_TITLE } from './ImportChatsCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const IDLE = { phase: 'idle' as const, agent: null, chatsDone: 0, chatsTotal: 0, items: 0, unchanged: 0, tooBig: 0, unreadable: 0, message: null };

function state(over: Partial<ImportState> = {}): ImportState {
  return {
    asked: false,
    sinceDays: 365,
    indexEnabled: true,
    agents: [{ id: 'claude', label: 'Claude Code', fromDefinition: false, enabled: true }],
    progress: IDLE,
    imported: { chats: 0, items: 0 },
    ...over,
  };
}

function fakeApi(st: ImportState = state()): ImportApi {
  return {
    state: vi.fn(async () => st),
    look: vi.fn(async () => []),
    start: vi.fn(),
    cancel: vi.fn(),
    dismiss: vi.fn(async () => undefined),
    clear: vi.fn(),
    open: vi.fn(),
    resumePlan: vi.fn(),
  } as unknown as ImportApi;
}

/** Renders the card and lets its async reads finish. */
async function mount(api: ImportApi, welcomeDone: boolean) {
  await act(async () => root.render(<ImportChatsCard api={api} isWelcomeDone={async () => welcomeDone} />));
  await act(async () => undefined);
}

const card = () => host.querySelector<HTMLElement>('[data-testid="import-chats-card"]');
const buttonNamed = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text) ?? null;

describe('first-start card: Bring my old chats', () => {
  it('is hidden when the question was already answered', async () => {
    await mount(fakeApi(state({ asked: true })), true);
    expect(card()).toBeNull();
    expect(host.innerHTML).toBe('');
  });

  it('is hidden when chat search is off', async () => {
    await mount(fakeApi(state({ indexEnabled: false })), true);
    expect(card()).toBeNull();
  });

  it('is hidden until the welcome is finished or skipped', async () => {
    await mount(fakeApi(state()), false);
    expect(card()).toBeNull();
  });

  it('is hidden outside the desktop app (the state says asked)', async () => {
    const api = fakeApi(state({ asked: true }));
    await mount(api, true);
    expect(card()).toBeNull();
    expect(api.look).not.toHaveBeenCalled();
  });

  it('shows the calm card once, with the words from the spec, and reads no agent folder', async () => {
    const api = fakeApi();
    await mount(api, true);
    const el = card()!;
    expect(el.matches('[data-ib-card][data-kind="note"][data-info]')).toBe(true);
    expect(el.querySelector('.ib-title')?.textContent).toBe(IMPORT_CARD_TITLE);
    expect(el.querySelector('.ib-reason')?.textContent).toBe(IMPORT_CARD_BODY);
    expect(Array.from(el.querySelectorAll('button')).map((b) => b.textContent)).toEqual(['Find my chats', 'Not now']);
    expect(api.state).toHaveBeenCalled();
    expect(api.look).not.toHaveBeenCalled();
    expect(api.start).not.toHaveBeenCalled();
    expect(api.dismiss).not.toHaveBeenCalled();
  });

  it('"Not now" remembers the answer, hides the card, and reads nothing', async () => {
    const api = fakeApi();
    await mount(api, true);
    await act(async () => buttonNamed('Not now')!.click());
    expect(api.dismiss).toHaveBeenCalledTimes(1);
    expect(card()).toBeNull();
    expect(api.look).not.toHaveBeenCalled();
  });

  it('keeps the card and says why when "Not now" fails', async () => {
    const api = fakeApi();
    api.dismiss = vi.fn(async () => {
      throw new Error('The settings file is locked.');
    });
    await mount(api, true);
    await act(async () => buttonNamed('Not now')!.click());
    expect(card()).not.toBeNull();
    expect(card()!.querySelector('.ib-note')?.textContent).toBe('The settings file is locked.');
  });

  it('"Find my chats" opens the Bring panel in a sheet and still reads nothing by itself', async () => {
    const api = fakeApi();
    await mount(api, true);
    await act(async () => buttonNamed('Find my chats')!.click());
    const sheet = host.querySelector('[aria-modal="true"]');
    expect(sheet).not.toBeNull();
    expect(sheet?.getAttribute('aria-labelledby')).toBe('bc-panel-title');
    expect(sheet?.querySelector('#bc-panel-title')?.textContent).toBe('Bring my old chats');
    expect(api.look).not.toHaveBeenCalled();
  });

  it('closing the panel re-reads the answer, so a started run hides the card', async () => {
    const api = fakeApi();
    await mount(api, true);
    await act(async () => buttonNamed('Find my chats')!.click());
    // The run was started from the panel, which marks the question as answered.
    (api.state as ReturnType<typeof vi.fn>).mockResolvedValue(state({ asked: true }));
    const close = host.querySelector<HTMLButtonElement>('.bc-actions .ck-btn--plain');
    expect(close?.textContent).toBe('Close');
    await act(async () => close!.click());
    await act(async () => undefined);
    expect(host.querySelector('[aria-modal="true"]')).toBeNull();
    expect(card()).toBeNull();
  });
});
