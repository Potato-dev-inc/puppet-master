// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  chatImportApi,
  chatImportCancel,
  chatImportClear,
  chatImportDismiss,
  chatImportLook,
  chatImportOpen,
  chatImportResumePlan,
  chatImportStart,
  chatImportState,
  clearBringChatsRequest,
  clearImportedChatRequest,
  foundLine,
  importedChatKey,
  onImportProgress,
  progressLine,
  requestBringChats,
  requestImportedChat,
  SINCE_CHOICES,
  useBringChatsRequest,
  useImportedChatRequest,
  type BringChatsRequest,
  type ImportFound,
  type ImportedChatRequest,
  type ImportProgress,
} from './chatImport';

const invoke = vi.fn();
const listen = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...a) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => (listen as (...x: unknown[]) => unknown)(...a) }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const setTauri = (on: boolean) => {
  const w = window as unknown as { __TAURI_INTERNALS__?: object };
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

const PROGRESS: ImportProgress = {
  phase: 'reading',
  agent: 'codex',
  chatsDone: 31,
  chatsTotal: 96,
  items: 900,
  unchanged: 0,
  tooBig: 0,
  unreadable: 0,
  message: null,
};

const FOUND: ImportFound = {
  id: 'claude',
  label: 'Claude Code',
  fromDefinition: false,
  chats: 412,
  bytes: 38 * 1024 * 1024,
  oldestMs: new Date(2026, 2, 10).getTime(),
  newestMs: new Date(2026, 9, 27).getTime(),
  already: 0,
  tooBig: 0,
  places: ['~/.claude/projects'],
  canContinue: true,
};

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
  setTauri(true);
});

afterEach(() => {
  setTauri(false);
});

describe('command wrappers', () => {
  it('call each command by its name and args', async () => {
    invoke.mockResolvedValue({});
    await chatImportState();
    expect(invoke).toHaveBeenLastCalledWith('chat_import_state');

    invoke.mockResolvedValue({ found: [FOUND] });
    expect(await chatImportLook(30)).toEqual([FOUND]);
    expect(invoke).toHaveBeenLastCalledWith('chat_import_look', { sinceDays: 30 });

    invoke.mockResolvedValue(PROGRESS);
    expect(await chatImportStart(['claude', 'codex'], null)).toEqual(PROGRESS);
    expect(invoke).toHaveBeenLastCalledWith('chat_import_start', { agents: ['claude', 'codex'], sinceDays: null });

    await chatImportCancel();
    expect(invoke).toHaveBeenLastCalledWith('chat_import_cancel');

    await chatImportDismiss();
    expect(invoke).toHaveBeenLastCalledWith('chat_import_dismiss');

    invoke.mockResolvedValue({ enabled: true, items: 0, bytes: 0, importedChats: 0, importedItems: 0 });
    await chatImportClear();
    expect(invoke).toHaveBeenLastCalledWith('chat_import_clear');

    invoke.mockResolvedValue({ meta: { title: 'Hi', agent: 'codex', folder: 'proj', updatedMs: 1, cutShort: false }, items: [] });
    await chatImportOpen('codex', 'abc12345');
    expect(invoke).toHaveBeenLastCalledWith('chat_import_open', { agent: 'codex', session: 'abc12345' });

    invoke.mockResolvedValue({ ok: true, agentType: 'claude', cwd: 'C:/x', extraArgs: ['--resume', 'abc12345'] });
    await chatImportResumePlan('claude', 'abc12345');
    expect(invoke).toHaveBeenLastCalledWith('chat_import_resume_plan', { agent: 'claude', session: 'abc12345' });
  });

  it('exposes the same wrappers as the api object', () => {
    expect(chatImportApi.look).toBe(chatImportLook);
    expect(chatImportApi.start).toBe(chatImportStart);
    expect(chatImportApi.resumePlan).toBe(chatImportResumePlan);
  });

  it('touches no command outside the desktop app and gives safe answers', async () => {
    setTauri(false);
    expect((await chatImportState()).asked).toBe(true);
    expect(await chatImportLook(null)).toEqual([]);
    expect((await chatImportStart(['claude'], null)).phase).toBe('idle');
    expect((await chatImportClear()).importedChats).toBe(0);
    expect((await chatImportResumePlan('claude', 'abc12345')).ok).toBe(false);
    await expect(chatImportOpen('claude', 'abc12345')).rejects.toThrow('desktop app');
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('onImportProgress', () => {
  it('passes each progress event on and stops listening when asked', async () => {
    const unlisten = vi.fn();
    let handler: (event: { payload: ImportProgress }) => void = () => {};
    listen.mockImplementation((name: string, h: typeof handler) => {
      expect(name).toBe('chat-import-progress');
      handler = h;
      return Promise.resolve(unlisten);
    });
    const seen = vi.fn();
    const stop = onImportProgress(seen);
    await flush();
    handler({ payload: PROGRESS });
    expect(seen).toHaveBeenCalledWith(PROGRESS);
    stop();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('does nothing outside the desktop app', () => {
    setTauri(false);
    const stop = onImportProgress(() => {});
    stop();
    expect(listen).not.toHaveBeenCalled();
  });
});

describe('line helpers', () => {
  const now = new Date(2026, 9, 28).getTime();

  it('foundLine shows the name, the chats, the size and the span', () => {
    expect(foundLine(FOUND, now)).toBe('Claude Code · 412 chats · 38 MB · Mar 2026 to now');
  });

  it('foundLine says singular, leaves out an unknown span, and names an older end month', () => {
    const one: ImportFound = { ...FOUND, label: 'Codex', chats: 1, bytes: 512, oldestMs: null, newestMs: null };
    expect(foundLine(one, now)).toBe('Codex · 1 chat · 512 B');
    const old = { ...FOUND, newestMs: new Date(2026, 7, 14).getTime() };
    expect(foundLine(old, now)).toBe('Claude Code · 412 chats · 38 MB · Mar 2026 to Aug 2026');
    const same = { ...FOUND, oldestMs: new Date(2026, 7, 2).getTime(), newestMs: new Date(2026, 7, 14).getTime() };
    expect(foundLine(same, now)).toBe('Claude Code · 412 chats · 38 MB · Aug 2026');
  });

  it('progressLine reads the phase, with the agent name or its id', () => {
    expect(progressLine(PROGRESS, 'Codex')).toBe('Reading Codex chats… 31 of 96');
    expect(progressLine(PROGRESS)).toBe('Reading codex chats… 31 of 96');
    expect(progressLine({ ...PROGRESS, phase: 'looking' })).toBe('Looking…');
    expect(progressLine({ ...PROGRESS, phase: 'failed', message: null })).toBe('Could not read the old chats.');
    expect(progressLine({ ...PROGRESS, phase: 'idle' })).toBe('');
  });

  it('keeps the since choices and the key of an old chat', () => {
    expect(SINCE_CHOICES.map((c) => c.label)).toEqual(['30 days', '90 days', '12 months', 'All']);
    expect(SINCE_CHOICES.map((c) => c.days)).toEqual([30, 90, 365, null]);
    expect(importedChatKey('codex', 'abc12345')).toBe('import:codex:abc12345');
  });
});

describe('request stores', () => {
  let host: HTMLDivElement;
  let root: Root;
  const seen: { imported: ImportedChatRequest | null; bring: BringChatsRequest | null } = { imported: null, bring: null };

  function Probe() {
    seen.imported = useImportedChatRequest();
    seen.bring = useBringChatsRequest();
    return null;
  }

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
    act(() => root.render(<Probe />));
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    seen.imported = null;
    seen.bring = null;
  });

  it('delivers an old-chat request once and clears it', () => {
    act(() => requestImportedChat('codex', 'abc12345', 'item:1'));
    expect(seen.imported).toMatchObject({ agent: 'codex', session: 'abc12345', itemId: 'item:1' });
    const req = seen.imported!;
    act(() => clearImportedChatRequest(req));
    expect(seen.imported).toBeNull();
    // A second clear with the same request does nothing.
    act(() => clearImportedChatRequest(req));
    expect(seen.imported).toBeNull();
  });

  it('keeps a newer request when an older one is cleared', () => {
    act(() => requestImportedChat('codex', 'aaaaaaaa'));
    const older = seen.imported!;
    act(() => requestImportedChat('claude', 'bbbbbbbb', 'item:2'));
    act(() => clearImportedChatRequest(older));
    expect(seen.imported).toMatchObject({ agent: 'claude', session: 'bbbbbbbb', itemId: 'item:2' });
  });

  it('delivers the bring-chats request once and clears it', () => {
    act(() => requestBringChats());
    expect(seen.bring).not.toBeNull();
    const req = seen.bring!;
    act(() => clearBringChatsRequest(req));
    expect(seen.bring).toBeNull();
  });
});
