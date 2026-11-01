// @vitest-environment jsdom
// "Add chat (ACP)": detect -> best guess -> confirm (no typing) -> set with confirmed true; no guess / error / timeout -> form; a refusal shows the backend's text; the
// Settings rows (add / remove / reason) and the "+" menu hint (only for agents that can get a chat, Labs on).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpLaneAgent, AcpLaneView, OverlayDetect, OverlayOption, OverlayProgress } from '../../../lib/acpChat';
import { addableChat, authorsText, firstAvailable, normalizeLaneView, normalizeOptions, registrySentence } from '../../../lib/acpChat';
import { AddChatDialog, commandLine, confirmSentence, DETECT_TIMEOUT_MS } from './AddChatDialog';
import { AgentLaneRows } from './AgentLaneRows';
import { NewAgentMenu } from '../../cockpit/NewAgentMenu';
import type { AccountsApi } from '../../../lib/accounts';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const detected: OverlayDetect = {
  ok: true,
  command: 'fake-agent',
  candidates: [
    { command: 'fake-agent', args: ['acp', '--quiet'], evidence: 'acp  Start the chat server', confidence: 'high' },
    { command: 'fake-bridge', args: ['--stdio'], evidence: 'fake-bridge  over stdio', confidence: 'medium' },
  ],
  stdio_hint: null,
  mentions_protocol: true,
};
const row = (over: Partial<AcpLaneAgent> = {}): AcpLaneAgent => ({ id: 'fake', label: 'Fake', lane: 'terminal', effective_lane: 'terminal', disabled: false, disabled_note: null, ...over });

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const q = <T extends Element = HTMLElement>(sel: string) => document.querySelector<T>(sel);
const byText = (text: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text);
const click = async (el: Element | null | undefined) => {
  expect(el, 'element to click').toBeTruthy();
  await act(async () => (el as HTMLElement).click());
};
const flush = async () => act(async () => void (await Promise.resolve()));

async function open(api: Parameters<typeof AddChatDialog>[0]['api'], onAdded = vi.fn(), onClose = vi.fn()) {
  await act(async () => root.render(<AddChatDialog agent={{ id: 'fake', label: 'Fake' }} api={api} onClose={onClose} onAdded={onAdded} />));
  await flush();
  return { onAdded, onClose };
}

const setField = async (id: string, value: string) =>
  act(async () => {
    const el = q<HTMLInputElement | HTMLTextAreaElement>(id)!;
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value')?.set?.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });

describe('AddChatDialog', () => {
  it('detects, picks the best guess and shows the confirm screen at once (nothing to type)', async () => {
    const overlayDetect = vi.fn(async () => ({
      ...detected,
      // the medium one comes first: the high one must still win
      candidates: [detected.candidates[1], detected.candidates[0]],
    }));
    const overlaySet = vi.fn(async () => row({ overlay: true }));
    const { onAdded } = await open({ overlayDetect, overlaySet });
    expect(overlayDetect).toHaveBeenCalledWith('fake');
    expect(overlayDetect).toHaveBeenCalledTimes(1);
    expect(q('[data-testid="add-chat-confirm"]')).toBeTruthy();
    expect(q('[data-testid="acp-step"]')).toBeNull();
    expect(q('[data-testid="add-chat-command"]')?.textContent).toBe('fake-agent acp --quiet');
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toBe('Fake will start fake-agent acp --quiet for chat.');
    expect(document.body.textContent).toContain(confirmSentence('Fake'));
    expect(overlaySet).not.toHaveBeenCalled();
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'fake-agent', args: ['acp', '--quiet'], env_allow: [] }, true);
    expect(onAdded).toHaveBeenCalledTimes(1);
  });

  it('Change details opens the form pre-filled with the guess and Review comes back to confirm', async () => {
    const overlaySet = vi.fn(async () => row());
    await open({ overlayDetect: async () => detected, overlaySet });
    await click(q('[data-testid="add-chat-change"]'));
    expect(q('[data-testid="acp-step"]')).toBeTruthy();
    expect(q<HTMLInputElement>('#acp-command')?.value).toBe('fake-agent');
    expect(q<HTMLTextAreaElement>('#acp-args')?.value).toBe('acp\n--quiet');
    expect(q('[data-testid="add-chat-note"]')).toBeNull();
    await setField('#acp-args', 'serve\n--acp');
    await click(byText('Review'));
    expect(q('[data-testid="add-chat-command"]')?.textContent).toBe('fake-agent serve --acp');
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'fake-agent', args: ['serve', '--acp'], env_allow: [] }, true);
  });

  it('finding nothing says so plainly and shows the form; a typed command works', async () => {
    const overlaySet = vi.fn(async () => row());
    await open({ overlayDetect: async () => ({ ok: true, command: 'x', candidates: [] }), overlaySet });
    expect(q('[data-testid="add-chat-confirm"]')).toBeNull();
    expect(q('[data-testid="add-chat-note"]')?.textContent).toBe('Could not find how Fake offers chat. You can type it in.');
    expect(q('[data-testid="acp-step"]')).toBeTruthy();
    await setField('#acp-command', 'my-agent');
    await setField('#acp-args', 'serve\n--acp');
    await setField('#acp-env', 'MY_KEY');
    await click(byText('Review'));
    expect(commandLine({ command: 'my-agent', args: ['serve', '--acp'], env_allow: [] })).toBe('my-agent serve --acp');
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'my-agent', args: ['serve', '--acp'], env_allow: ['MY_KEY'] }, true);
  });

  it('a failed look-up shows the message and the form', async () => {
    await open({ overlayDetect: async () => Promise.reject(new Error('WIZARD_NOT_FOUND: `fake` was not found on PATH.')), overlaySet: vi.fn() });
    expect(q('[data-testid="add-chat-reading"]')).toBeNull();
    expect(q('[data-testid="add-chat-note"]')?.textContent).toContain('Could not find how Fake offers chat. You can type it in.');
    expect(q('[data-testid="acp-step"]')).toBeTruthy();
  });

  it('gives up after 20 seconds so "Reading the help text..." can never hang', async () => {
    vi.useFakeTimers();
    try {
      const overlayDetect = vi.fn(() => new Promise<OverlayDetect>(() => {}));
      await open({ overlayDetect, overlaySet: vi.fn() });
      expect(q('[data-testid="add-chat-reading"]')).toBeTruthy();
      await act(async () => void (await vi.advanceTimersByTimeAsync(DETECT_TIMEOUT_MS - 1)));
      expect(q('[data-testid="add-chat-reading"]')).toBeTruthy();
      await act(async () => void (await vi.advanceTimersByTimeAsync(2)));
      expect(q('[data-testid="add-chat-reading"]')).toBeNull();
      expect(q('[data-testid="add-chat-note"]')?.textContent).toContain('You can type it in.');
      expect(q('[data-testid="acp-step"]')).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows the backend error text and stays open when it refuses', async () => {
    const overlaySet = vi.fn(async () => {
      throw new Error('ACP_OVERLAY_REFUSED: That program is a shell, which is not allowed.');
    });
    const { onAdded } = await open({ overlayDetect: async () => detected, overlaySet });
    await click(byText('Add chat'));
    expect(q('[data-testid="add-chat-error"]')?.textContent).toBe('That program is a shell, which is not allowed.');
    expect(onAdded).not.toHaveBeenCalled();
    expect(byText('Add chat')?.disabled).toBe(false);
  });

  it('Cancel on the confirm screen closes without saving', async () => {
    const overlaySet = vi.fn();
    const { onClose } = await open({ overlayDetect: async () => detected, overlaySet });
    await click(byText('Cancel'));
    expect(onClose).toHaveBeenCalled();
    expect(overlaySet).not.toHaveBeenCalled();
  });
});

const nativeOpt = (over: Partial<OverlayOption> = {}): OverlayOption => ({
  kind: 'native',
  label: 'fake on this computer',
  summary: 'The program you already have starts chat itself.',
  command_line: 'fake-agent acp',
  acp: { command: 'fake-agent', args: ['acp'], env_allow: [] },
  available: true,
  ...over,
});
const registryOpt = (over: Partial<OverlayOption> = {}): OverlayOption => ({
  kind: 'registry',
  label: 'Fake Adapter 1.2.3',
  summary: 'An adapter that speaks ACP.',
  registry: { id: 'fake-acp', name: 'Fake Adapter', version: '1.2.3', authors: ['Alice', 'Bob'], license: 'MIT', repository: 'https://example.test/fake', distribution: 'npx', size_hint: '40 MB' },
  available: true,
  ...over,
});
const bridgeOpt = (): OverlayOption => ({ kind: 'bridge', label: 'Chat made by Puppet Master', summary: 'Puppet Master makes a chat adapter.', available: false, reason: 'Fake has no non-interactive mode, so a chat adapter cannot be made' });
const bridgeOn = (hasMemory: boolean): OverlayOption => ({ kind: 'bridge', label: 'Chat made by Puppet Master', summary: 'Puppet Master will make a chat adapter for Fake.', bridge: { has_memory: hasMemory }, available: true });
const resolved = (...options: OverlayOption[]) => vi.fn(async () => ({ ok: true, options }));

describe('AddChatDialog with the app resolving the best way (native, registry, bridge)', () => {
  it('a program that starts chat itself: one confirm screen, saved with overlaySet, no detect call', async () => {
    const overlayResolve = resolved(nativeOpt(), bridgeOpt());
    const overlayDetect = vi.fn();
    const overlaySet = vi.fn(async () => row({ overlay: true }));
    const { onAdded } = await open({ overlayResolve, overlayDetect, overlaySet });
    expect(overlayResolve).toHaveBeenCalledTimes(1);
    expect(overlayDetect).not.toHaveBeenCalled();
    expect(q('[data-testid="add-chat-options"]')).toBeNull(); // one usable way: no choice to make
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toBe('Fake will start fake-agent acp for chat.');
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'fake-agent', args: ['acp'], env_allow: [] }, true);
    expect(onAdded).toHaveBeenCalledTimes(1);
  });

  it('a registry adapter: says who made it and what happens, downloads with progress, then shows success', async () => {
    const overlayResolve = resolved(registryOpt(), bridgeOpt());
    let report: (p: OverlayProgress) => void = () => {};
    const stop = vi.fn();
    const overlayProgress = vi.fn(async (_agent: string, cb: (p: OverlayProgress) => void) => {
      report = cb;
      return stop;
    });
    let finish: () => void = () => {};
    const overlayInstall = vi.fn(() => new Promise<unknown>((resolve) => (finish = () => resolve({}))));
    const overlaySet = vi.fn();
    const { onAdded } = await open({ overlayResolve, overlayDetect: vi.fn(), overlaySet, overlayInstall, overlayProgress });
    const text = q('[data-testid="add-chat-summary"]')?.textContent;
    expect(text).toBe('Puppet Master will download Fake Adapter 1.2.3 by Alice, Bob (MIT) from the official ACP registry and run it for chat. About 40 MB.');
    expect(q('[data-testid="add-chat-registry-about"]')?.textContent).toBe('An adapter that speaks ACP.');
    expect(overlayInstall).not.toHaveBeenCalled();
    await click(byText('Add chat'));
    expect(overlayInstall).toHaveBeenCalledWith('fake', { kind: 'registry', id: 'fake-acp', version: '1.2.3' }, true);
    expect(q('[data-testid="add-chat-installing"]')).toBeTruthy();
    expect(byText('Cancel')).toBeUndefined();
    await act(async () => report({ state: 'installing', lines: ['Downloading Fake Adapter 1.2.3...', 'added 1 package'], detail: '' }));
    expect(q('[data-testid="add-chat-progress"]')?.textContent).toContain('added 1 package');
    await act(async () => finish());
    await flush();
    expect(q('[data-testid="add-chat-success"]')?.textContent).toBe('Fake Adapter 1.2.3 is ready. Chat is added for Fake.');
    expect(stop).toHaveBeenCalled();
    expect(overlaySet).not.toHaveBeenCalled();
    expect(onAdded).not.toHaveBeenCalled();
    await click(byText('Done'));
    expect(onAdded).toHaveBeenCalledWith(null);
  });

  it('registry text is shown as plain text, never as markup', async () => {
    const evil = registryOpt({ summary: '<img src=x onerror=alert(1)>', registry: { ...registryOpt().registry!, name: '<b>Bold</b>', authors: ['<i>x</i>'] } });
    await open({ overlayResolve: resolved(evil), overlaySet: vi.fn(), overlayInstall: vi.fn() });
    expect(document.querySelector('[data-testid="add-chat-dialog"] img, [data-testid="add-chat-dialog"] b, [data-testid="add-chat-dialog"] i')).toBeNull();
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toContain('<b>Bold</b>');
  });

  it('two usable ways are radio choices, best first; an unavailable bridge is never one', async () => {
    const overlayResolve = resolved(nativeOpt(), registryOpt(), bridgeOpt());
    const overlaySet = vi.fn(async () => row());
    const overlayInstall = vi.fn(async () => ({}));
    await open({ overlayResolve, overlaySet, overlayInstall });
    const radios = [...document.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
    expect(radios).toHaveLength(2);
    expect(radios.map((r) => r.checked)).toEqual([true, false]);
    expect(q('[data-testid="add-chat-command"]')?.textContent).toBe('fake-agent acp');
    await click(radios[1]);
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toContain('will download Fake Adapter 1.2.3');
    await click(radios[0]);
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledTimes(1);
    expect(overlayInstall).not.toHaveBeenCalled();
  });

  it('the bridge is the last resort: alone it is the confirm screen, with the honest sentence, and saves with kind bridge', async () => {
    const overlayInstall = vi.fn(async () => row({ overlay: true }));
    const overlaySet = vi.fn();
    const { onAdded } = await open({ overlayResolve: resolved(nativeOpt({ available: false, reason: 'no' }), bridgeOn(true)), overlaySet, overlayInstall });
    expect(q('[data-testid="add-chat-options"]')).toBeNull();
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toBe(
      'Fake has no chat support of its own, so Puppet Master will make one. It talks to Fake one message at a time: text only, no tool cards, it remembers the conversation.',
    );
    await click(byText('Add chat'));
    await flush();
    expect(overlayInstall).toHaveBeenCalledWith('fake', { kind: 'bridge' }, true);
    expect(overlaySet).not.toHaveBeenCalled();
    expect(onAdded).toHaveBeenCalledTimes(1);
  });

  it('the bridge says plainly when the agent does not remember earlier messages, and a failure keeps the screen', async () => {
    const overlayInstall = vi.fn().mockRejectedValueOnce(new Error('NOT_AVAILABLE: Fake has no non-interactive mode, so a chat adapter cannot be made'));
    await open({ overlayResolve: resolved(bridgeOn(false)), overlaySet: vi.fn(), overlayInstall });
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toContain('text only, no tool cards, it does not remember earlier messages.');
    await click(byText('Add chat'));
    await flush();
    expect(q('[data-testid="add-chat-error"]')?.textContent).toContain('cannot be made');
    expect(q('[data-testid="add-chat-confirm"]')).toBeTruthy();
    expect(byText('Add chat')?.disabled).toBe(false);
  });

  it('a better way stays first and the bridge is the last radio', async () => {
    const overlayInstall = vi.fn(async () => row({ overlay: true }));
    await open({ overlayResolve: resolved(nativeOpt(), bridgeOn(true)), overlaySet: vi.fn(), overlayInstall });
    const radios = [...document.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
    expect(radios.map((r) => r.checked)).toEqual([true, false]);
    await click(radios[1]);
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toContain('so Puppet Master will make one');
    await click(byText('Add chat'));
    await flush();
    expect(overlayInstall).toHaveBeenCalledWith('fake', { kind: 'bridge' }, true);
  });

  it('the first usable way is preselected even when an unusable one is listed first', async () => {
    await open({ overlayResolve: resolved(nativeOpt({ available: false, reason: 'Its help does not show a chat (ACP) command.' }), registryOpt(), bridgeOpt()), overlaySet: vi.fn(), overlayInstall: vi.fn() });
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toContain('will download');
    expect(q('[data-testid="add-chat-options"]')).toBeNull();
  });

  it('a failed download shows the sentence, stays on the confirm screen and can be tried again', async () => {
    const overlayInstall = vi.fn().mockRejectedValueOnce(new Error('ACP_ADDON_FAILED: The download failed: could not reach the server.')).mockResolvedValueOnce({});
    await open({ overlayResolve: resolved(registryOpt()), overlaySet: vi.fn(), overlayInstall });
    await click(byText('Add chat'));
    await flush();
    expect(q('[data-testid="add-chat-error"]')?.textContent).toBe('The download failed: could not reach the server.');
    expect(q('[data-testid="add-chat-confirm"]')).toBeTruthy();
    expect(byText('Add chat')?.disabled).toBe(false);
    await click(byText('Add chat'));
    await flush();
    expect(q('[data-testid="add-chat-success"]')).toBeTruthy();
    expect(q('[data-testid="add-chat-error"]')).toBeNull();
  });

  it('nothing usable: says why, shows the form, and a typed command still works', async () => {
    const overlaySet = vi.fn(async () => row());
    await open({ overlayResolve: resolved(nativeOpt({ available: false, reason: 'Its help does not show a chat (ACP) command.' }), bridgeOpt()), overlaySet });
    expect(q('[data-testid="add-chat-confirm"]')).toBeNull();
    expect(q('[data-testid="add-chat-note"]')?.textContent).toBe('Could not find how Fake offers chat. You can type it in. (Its help does not show a chat (ACP) command.)');
    await setField('#acp-command', 'my-agent');
    await setField('#acp-args', 'serve');
    await click(byText('Review'));
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'my-agent', args: ['serve'], env_allow: [] }, true);
  });

  it('Change details on a native way pre-fills the form; on a download it starts blank', async () => {
    await open({ overlayResolve: resolved(nativeOpt()), overlaySet: vi.fn() });
    await click(q('[data-testid="add-chat-change"]'));
    expect(q<HTMLInputElement>('#acp-command')?.value).toBe('fake-agent');
    expect(q<HTMLTextAreaElement>('#acp-args')?.value).toBe('acp');
    await act(async () => root.render(<div />));
    await open({ overlayResolve: resolved(registryOpt()), overlaySet: vi.fn(), overlayInstall: vi.fn() });
    await click(q('[data-testid="add-chat-change"]'));
    expect(q<HTMLInputElement>('#acp-command')?.value).toBe('');
  });

  it('an app whose resolve fails falls back to reading the help text', async () => {
    const overlayResolve = vi.fn(async () => Promise.reject(new Error('unknown command acp_overlay_resolve')));
    const overlayDetect = vi.fn(async () => detected);
    const overlaySet = vi.fn(async () => row());
    await open({ overlayResolve, overlayDetect, overlaySet });
    expect(overlayDetect).toHaveBeenCalledWith('fake');
    expect(q('[data-testid="add-chat-command"]')?.textContent).toBe('fake-agent acp --quiet');
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'fake-agent', args: ['acp', '--quiet'], env_allow: [] }, true);
  });

  it('Cancel on a download confirm closes without installing', async () => {
    const overlayInstall = vi.fn();
    const { onClose } = await open({ overlayResolve: resolved(registryOpt()), overlaySet: vi.fn(), overlayInstall });
    await click(byText('Cancel'));
    expect(onClose).toHaveBeenCalled();
    expect(overlayInstall).not.toHaveBeenCalled();
  });
});

describe('overlay option helpers', () => {
  it('writes the confirm sentence with and without optional parts', () => {
    const info = registryOpt().registry!;
    expect(registrySentence(info)).toBe('Puppet Master will download Fake Adapter 1.2.3 by Alice, Bob (MIT) from the official ACP registry and run it for chat. About 40 MB.');
    expect(registrySentence({ ...info, authors: [], license: '', size_hint: null })).toBe('Puppet Master will download Fake Adapter 1.2.3 from the official ACP registry and run it for chat.');
    expect(authorsText(['A', 'B', 'C', 'D'])).toBe('A, B, C and others');
  });

  it('picks the first usable option and drops unknown kinds', () => {
    expect(firstAvailable([nativeOpt({ available: false }), registryOpt(), bridgeOpt()])).toBe(1);
    expect(firstAvailable([bridgeOpt()])).toBe(-1);
    expect(firstAvailable(null)).toBe(-1);
    const list = normalizeOptions({ ok: true, options: [registryOpt(), { kind: 'mystery', label: 'x', summary: '', available: true } as unknown as OverlayOption] });
    expect(list.map((o) => o.kind)).toEqual(['registry']);
    expect(normalizeOptions(null)).toEqual([]);
  });
});

describe('lane view parsing', () => {
  it('defaults the new fields for an old backend', () => {
    const v = normalizeLaneView({ labs_enabled: true, agents: [row()] });
    expect(v.can_add_chat).toEqual([]);
    expect(addableChat(v)).toEqual([]);
    expect(addableChat({ labs_enabled: false, agents: [], can_add_chat: [{ id: 'a', label: 'A' }] })).toEqual([]);
  });
});

describe('Settings How to run rows', () => {
  const view = (): AcpLaneView => ({
    labs_enabled: true,
    agents: [row({ id: 'mine', label: 'Mine', overlay: true })],
    can_add_chat: [
      { id: 'fake', label: 'Fake' },
      { id: 'blocked', label: 'Blocked', reason: 'It has no help text to read.' },
    ],
  });

  it('add button opens the dialog; a success refreshes the list', async () => {
    let calls = 0;
    const laneGet = vi.fn(async () => (calls++ === 0 ? view() : { labs_enabled: true, agents: [row({ id: 'fake', overlay: true })], can_add_chat: [] }));
    const overlaySet = vi.fn(async () => row({ overlay: true }));
    await act(async () => root.render(<AgentLaneRows api={{ laneGet, laneSet: vi.fn(), overlayDetect: async () => detected, overlaySet, overlayRemove: vi.fn() }} />));
    await flush();
    const blocked = document.querySelector<HTMLButtonElement>('[aria-label="Add chat (ACP) for Blocked"]');
    expect(blocked?.disabled).toBe(true);
    expect(document.body.textContent).toContain('It has no help text to read.');
    await click(document.querySelector('[aria-label="Add chat (ACP) for Fake"]'));
    await flush();
    await click(byText('Add chat'));
    await flush();
    expect(overlaySet).toHaveBeenCalledWith('fake', expect.anything(), true);
    expect(laneGet).toHaveBeenCalledTimes(2);
    expect(q('[data-testid="add-chat-dialog"]')).toBeNull();
    expect(document.querySelector('[aria-label="Add chat (ACP) for Fake"]')).toBeNull();
  });

  it('Remove chat shows on every agent with a chat, asks first, and an older app removes only an added chat', async () => {
    const overlayRemove = vi.fn(async () => undefined);
    const laneGet = vi.fn(async () => view());
    await act(async () => root.render(<AgentLaneRows api={{ laneGet, laneSet: vi.fn(), overlayRemove }} />));
    await flush();
    expect(document.querySelectorAll('[aria-label^="Remove chat for"]').length).toBeGreaterThan(0);
    await click(document.querySelector('[aria-label="Remove chat for Mine"]'));
    expect(overlayRemove).not.toHaveBeenCalled();
    await click(byText('Remove'));
    await flush();
    expect(overlayRemove).toHaveBeenCalledWith('mine');
    expect(laneGet).toHaveBeenCalledTimes(2);
  });
});

describe('the + menu hint', () => {
  const accounts = { list: async () => ({ agents: [], accounts: [] }) } as unknown as AccountsApi;
  const entries = [
    { type: 'fake', label: 'Fake' },
    { type: 'other', label: 'Other' },
  ];
  async function menu(chat: Parameters<typeof NewAgentMenu>[0]['chat']) {
    await act(async () => root.render(<NewAgentMenu entries={entries} readyAgents={new Set()} onStart={vi.fn()} onClose={vi.fn()} accountsApi={accounts} chat={chat} />));
  }

  it('shows "Set up chat" only next to an agent in can_add_chat and calls onSetUp', async () => {
    const onSetUp = vi.fn();
    const add = { id: 'fake', label: 'Fake' };
    await menu({ agents: [], onStart: vi.fn(async () => undefined), canAdd: [add], onSetUp });
    expect(document.querySelectorAll('.ck-menu-setup')).toHaveLength(1);
    expect(host.querySelector('[data-testid="setup-row-other"]')).toBeNull();
    await click(host.querySelector('[aria-label="Set up chat for Fake"]'));
    expect(onSetUp).toHaveBeenCalledWith(add);
  });

  it('shows no hint when nothing can be added (Labs off sends an empty list)', async () => {
    await menu({
      agents: [],
      onStart: vi.fn(async () => undefined),
      canAdd: addableChat({ labs_enabled: false, agents: [], can_add_chat: [{ id: 'fake', label: 'Fake' }] }),
      onSetUp: vi.fn(),
    });
    expect(host.querySelector('.ck-menu-setup')).toBeNull();
    await menu(undefined);
    expect(host.querySelector('.ck-menu-setup')).toBeNull();
  });
});

describe('AddChatDialog with Chat (terminal)', () => {
  const termSummary = 'Puppet Master runs Fake in a terminal and shows it as a chat. You get its tool cards and history, and you can answer its questions here. It cannot resume old sessions.';
  const termOpt = (over: Partial<OverlayOption> = {}): OverlayOption => ({
    kind: 'terminal_chat',
    label: 'Chat (terminal)',
    summary: termSummary,
    terminal_chat: { reads_file: true, can_resume: false, can_answer_prompts: true, note: '' },
    available: true,
    ...over,
  });

  it('is kept by normalizeOptions and ranked after native and registry, before the bridge', () => {
    const list = normalizeOptions({ ok: true, options: [bridgeOpt(), termOpt(), registryOpt()] });
    expect(list.map((o) => o.kind)).toEqual(['bridge', 'terminal_chat', 'registry']);
    expect(firstAvailable([nativeOpt({ available: false }), termOpt(), bridgeOpt()])).toBe(1);
  });

  it('alone it is the confirm screen with its summary and "Nothing to download.", no command box, no ACP word', async () => {
    await open({ overlayResolve: resolved(termOpt()), overlaySet: vi.fn(async () => row()), overlayInstall: vi.fn(async () => ({})) });
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toBe(termSummary);
    expect(q('[data-testid="add-chat-command"]')).toBeNull();
    expect(document.body.textContent).toContain('Nothing to download.');
    expect(document.querySelector('[data-testid="add-chat-dialog"]')?.textContent).not.toMatch(/ACP/);
  });

  it('is a radio with its label and summary when another route is also usable, and is preselected when first', async () => {
    await open({ overlayResolve: resolved(termOpt(), registryOpt()), overlaySet: vi.fn(async () => row()), overlayInstall: vi.fn(async () => ({})) });
    const radios = [...document.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
    expect(radios).toHaveLength(2);
    expect(radios.map((r) => r.checked)).toEqual([true, false]);
    const termRadio = q('[data-testid="add-chat-option-terminal_chat"]')!;
    expect(termRadio.textContent).toContain('Chat (terminal)');
    expect(termRadio.textContent).toContain(termSummary);
    expect(termRadio.textContent).toContain('Nothing to download.');
    expect(termRadio.textContent).not.toMatch(/ACP/);
  });

  it('is not preselected when a native program comes first', async () => {
    await open({ overlayResolve: resolved(nativeOpt(), termOpt()), overlaySet: vi.fn(async () => row()), overlayInstall: vi.fn(async () => ({})) });
    const radios = [...document.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
    expect(radios.map((r) => r.checked)).toEqual([true, false]);
  });

  it('Add chat calls install with {kind:"terminal_chat"} and confirmed true, never overlaySet, and reports the row', async () => {
    const overlayInstall = vi.fn(async () => row({ lane: 'terminal_chat' as never }));
    const overlaySet = vi.fn(async () => row());
    const { onAdded } = await open({ overlayResolve: resolved(termOpt()), overlaySet, overlayInstall });
    await click(byText('Add chat'));
    await flush();
    expect(overlayInstall).toHaveBeenCalledWith('fake', { kind: 'terminal_chat' }, true);
    expect(overlaySet).not.toHaveBeenCalled();
    expect(onAdded).toHaveBeenCalledTimes(1);
  });

  it('a refusal shows the backend text and keeps the dialog open', async () => {
    const overlayInstall = vi.fn(async () => {
      throw new Error('Turn on Labs first.');
    });
    const { onAdded } = await open({ overlayResolve: resolved(termOpt()), overlayInstall });
    await click(byText('Add chat'));
    await flush();
    expect(q('[data-testid="add-chat-error"]')?.textContent).toContain('Turn on Labs first.');
    expect(onAdded).not.toHaveBeenCalled();
  });

  it('other routes are unchanged: a native program still saves with overlaySet, not install', async () => {
    const overlaySet = vi.fn(async () => row());
    const overlayInstall = vi.fn(async () => ({}));
    await open({ overlayResolve: resolved(nativeOpt(), termOpt()), overlaySet, overlayInstall });
    await click(byText('Add chat'));
    await flush();
    expect(overlaySet).toHaveBeenCalledWith('fake', expect.objectContaining({ command: 'fake-agent' }), true);
    expect(overlayInstall).not.toHaveBeenCalled();
  });
});
