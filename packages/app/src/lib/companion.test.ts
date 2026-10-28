// @vitest-environment jsdom
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { makeBridgeClient } from './bridge';
import type { BridgeClient } from './bridge-types';
import {
  bridgeCompanionTransport,
  fakeCompanionTransport,
  formatWhen,
  stateWord,
  tauriCompanionTransport,
  type CheckIn,
  type CompanionDraft,
  type CompanionState,
  type CompanionView,
  type When,
} from './companion';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const BASE = 'http://127.0.0.1:17321';
const tauriWindow = () => window as unknown as { __TAURI_INTERNALS__?: unknown };

function view(over: Partial<CompanionView> = {}): CompanionView {
  return {
    id: 'mochi-1',
    name: 'Mochi',
    avatar: 'mochi',
    persona: 'calm',
    agentType: 'claude',
    agentLabel: 'Claude Code',
    folderName: 'mochi-1',
    state: 'idle',
    stateLine: 'Here. Ask me anything.',
    paneId: 'companion-mochi-1',
    needs: null,
    pendingMemory: 0,
    nextCheckin: null,
    restarts: 0,
    autoStart: true,
    ...over,
  };
}

const draft: CompanionDraft = {
  name: 'Bun',
  avatar: 'bit',
  persona: 'plain',
  agentType: 'claude',
  folder: 'C:\\Users\\you\\PuppetMaster\\companions\\bun',
};

describe('tauriCompanionTransport', () => {
  beforeEach(() => {
    tauriWindow().__TAURI_INTERNALS__ = {};
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });
  afterEach(() => {
    delete tauriWindow().__TAURI_INTERNALS__;
  });

  const checkins: CheckIn[] = [];
  it.each<[string, () => Promise<unknown>, string, Record<string, unknown>]>([
    ['list', () => tauriCompanionTransport.list(), 'companion_list', {}],
    ['get', () => tauriCompanionTransport.get('a'), 'companion_get', { id: 'a' }],
    ['wake', () => tauriCompanionTransport.wake('a'), 'companion_wake', { id: 'a' }],
    ['sleep', () => tauriCompanionTransport.sleep('a'), 'companion_sleep', { id: 'a' }],
    ['memory', () => tauriCompanionTransport.memory('a'), 'companion_memory_get', { id: 'a' }],
    [
      'decidePending',
      () => tauriCompanionTransport.decidePending('a', 'x1', 'keep'),
      'companion_pending_decide',
      { id: 'a', itemId: 'x1', decision: 'keep' },
    ],
    ['feed', () => tauriCompanionTransport.feed('a', 5), 'companion_feed', { id: 'a', sinceMs: 5 }],
    ['feed default', () => tauriCompanionTransport.feed('a'), 'companion_feed', { id: 'a', sinceMs: 0 }],
    ['agents', () => tauriCompanionTransport.agents(), 'companion_agents', {}],
    ['create', () => tauriCompanionTransport.create(draft), 'companion_create', { input: draft }],
    ['update', () => tauriCompanionTransport.update('a', { name: 'Bo' }), 'companion_update', { id: 'a', patch: { name: 'Bo' } }],
    ['archive', () => tauriCompanionTransport.archive('a'), 'companion_archive', { id: 'a' }],
    ['restart', () => tauriCompanionTransport.restart('a'), 'companion_restart', { id: 'a' }],
    [
      'saveMemory',
      () => tauriCompanionTransport.saveMemory('a', 'soul', 'Kind.'),
      'companion_memory_save',
      { id: 'a', file: 'soul', text: 'Kind.' },
    ],
    [
      'decideOutsideChange',
      () => tauriCompanionTransport.decideOutsideChange('a', 'user', 'undo'),
      'companion_outside_change_decide',
      { id: 'a', file: 'user', decision: 'undo' },
    ],
    ['tidyUp', () => tauriCompanionTransport.tidyUp('a'), 'companion_tidy_up', { id: 'a' }],
    [
      'approveSkill (as proposed)',
      () => tauriCompanionTransport.approveSkill('a', 'p1'),
      'companion_skill_approve',
      { id: 'a', itemId: 'p1', title: null, body: null },
    ],
    [
      'approveSkill (edited)',
      () => tauriCompanionTransport.approveSkill('a', 'p1', { title: 'Report', body: 'Steps.' }),
      'companion_skill_approve',
      { id: 'a', itemId: 'p1', title: 'Report', body: 'Steps.' },
    ],
    ['getSkill', () => tauriCompanionTransport.getSkill('a', 'report'), 'companion_skill_get', { id: 'a', slug: 'report' }],
    [
      'saveSkill',
      () => tauriCompanionTransport.saveSkill('a', 'report', 'Report', 'Steps.'),
      'companion_skill_save',
      { id: 'a', slug: 'report', title: 'Report', body: 'Steps.' },
    ],
    ['removeSkill', () => tauriCompanionTransport.removeSkill('a', 'report'), 'companion_skill_remove', { id: 'a', slug: 'report' }],
    [
      'setCheckIns',
      () => tauriCompanionTransport.setCheckIns('a', checkins),
      'companion_checkins_set',
      { id: 'a', checkins },
    ],
    ['openFolder', () => tauriCompanionTransport.openFolder('a'), 'companion_open_folder', { id: 'a' }],
  ])('%s calls %s', async (_name, run, command, args) => {
    await run();
    expect(invoke).toHaveBeenCalledWith(command, args);
  });

  it('says it needs the desktop shell when there is no Tauri', async () => {
    delete tauriWindow().__TAURI_INTERNALS__;
    await expect(tauriCompanionTransport.list()).rejects.toThrow('Desktop shell required: companion_list');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('passes a refusal from the backend on as a plain sentence', async () => {
    vi.mocked(invoke).mockRejectedValueOnce('This companion is not here any more.');
    await expect(tauriCompanionTransport.get('gone')).rejects.toThrow('This companion is not here any more.');
  });

  it('hears the companion://changed event with the whole list', async () => {
    const cb = vi.fn();
    vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
      (handler as (e: { payload: CompanionView[] }) => void)({ payload: [view()] });
      return () => {};
    });
    await tauriCompanionTransport.onChanged?.(cb);
    expect(listen).toHaveBeenCalledWith('companion://changed', expect.any(Function));
    expect(cb).toHaveBeenCalledWith([expect.objectContaining({ id: 'mochi-1' })]);
  });
});

describe('bridgeCompanionTransport routes', () => {
  let fetchMock: ReturnType<typeof vi.fn>;

  function answerWith(body: unknown, status = 200): void {
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));
  }

  beforeEach(() => {
    fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const transport = () => bridgeCompanionTransport(makeBridgeClient(BASE));
  const last = () => fetchMock.mock.calls.at(-1) as [string, RequestInit];

  it('reads the list from GET /companions and unwraps it', async () => {
    answerWith({ v: 1, companions: [view()] });
    const list = await transport().list();
    expect(last()[0]).toBe(`${BASE}/companions`);
    expect(last()[1]).toMatchObject({ method: 'GET' });
    expect(list.map((c) => c.id)).toEqual(['mochi-1']);
  });

  it('reads one companion, its memory and its feed', async () => {
    answerWith(view());
    await transport().get('mochi-1');
    expect(last()[0]).toBe(`${BASE}/companions/mochi-1`);

    answerWith({ soul: '', user: '', memory: '', caps: { soul: 1500, user: 1400, memory: 2200 }, pending: [], outsideChanges: [] });
    await transport().memory('mochi-1');
    expect(last()[0]).toBe(`${BASE}/companions/mochi-1/memory`);

    answerWith({ items: [{ id: 'f1', atMs: 9, kind: 'reply', text: 'Hi.' }] });
    const feed = await transport().feed('mochi-1', 5);
    expect(last()[0]).toBe(`${BASE}/companions/mochi-1/feed?since=5`);
    expect(feed).toHaveLength(1);
  });

  it('sends a pending decision and wake or sleep as POST', async () => {
    answerWith({ soul: '', user: '', memory: '', caps: { soul: 1, user: 1, memory: 1 }, pending: [], outsideChanges: [] });
    await transport().decidePending('mochi-1', 'x1', 'keep');
    expect(last()[0]).toBe(`${BASE}/companions/mochi-1/memory/pending/x1`);
    expect(last()[1]).toMatchObject({ method: 'POST', body: JSON.stringify({ decision: 'keep' }) });

    answerWith(view({ state: 'idle' }));
    await transport().wake('mochi-1');
    expect(last()[0]).toBe(`${BASE}/companions/mochi-1/wake`);

    answerWith(view({ state: 'sleeping' }));
    await transport().sleep('mochi-1');
    expect(last()[0]).toBe(`${BASE}/companions/mochi-1/sleep`);
    expect(last()[1]).toMatchObject({ method: 'POST' });
  });

  it('shows the PC plain reason when a route refuses', async () => {
    answerWith({ error: 'Companion not found.' }, 404);
    await expect(transport().get('gone')).rejects.toThrow('Companion not found.');
  });

  it('says the PC could not be reached when the network fails', async () => {
    fetchMock.mockRejectedValueOnce(new TypeError('Failed to fetch'));
    await expect(transport().list()).rejects.toThrow('Could not reach your PC.');
  });

  it('says an old PC cannot show companions', async () => {
    await expect(bridgeCompanionTransport({} as BridgeClient).list()).rejects.toThrow('This PC is too old');
  });

  it('refuses the desktop-only calls on the phone', async () => {
    await expect(transport().create(draft)).rejects.toThrow('Companions are set up on the computer.');
    await expect(transport().agents()).rejects.toThrow('Companions are set up on the computer.');
    await expect(transport().approveSkill('a', 'p1')).rejects.toThrow('Companions are set up on the computer.');
    await expect(transport().getSkill('a', 'report')).rejects.toThrow('Companions are set up on the computer.');
    await expect(transport().saveSkill('a', 'report', 'T', 'b')).rejects.toThrow('Companions are set up on the computer.');
    await expect(transport().removeSkill('a', 'report')).rejects.toThrow('Companions are set up on the computer.');
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe('formatWhen', () => {
  const cases: [When, string][] = [
    [{ kind: 'daily', at: { h: 8, m: 0 } }, 'Every day at 8:00'],
    [{ kind: 'weekdays', at: { h: 20, m: 30 } }, 'Weekdays at 20:30'],
    [{ kind: 'weekly', day: 0, at: { h: 8, m: 0 } }, 'Every Monday at 8:00'],
    [{ kind: 'weekly', day: 6, at: { h: 7, m: 5 } }, 'Every Sunday at 7:05'],
    [{ kind: 'every', minutes: 30 }, 'Every 30 minutes'],
    [{ kind: 'every', minutes: 60 }, 'Every hour'],
    [{ kind: 'every', minutes: 120 }, 'Every 2 hours'],
  ];
  it.each(cases)('formats %#: %o', (when, text) => {
    expect(formatWhen(when)).toBe(text);
  });

  it('reads a one-time check-in with its date and time', () => {
    const atMs = new Date(2026, 9, 28, 8, 5).getTime();
    expect(formatWhen({ kind: 'once', atMs })).toBe('Once on Oct 28 at 8:05');
  });
});

describe('stateWord', () => {
  it.each<[CompanionState, string]>([
    ['starting', 'Waking up'],
    ['idle', 'Here'],
    ['working', 'Working'],
    ['needs_you', 'Needs you'],
    ['sleeping', 'Asleep'],
    ['stopped', 'Stopped'],
  ])('%s reads "%s"', (state, word) => {
    expect(stateWord(state)).toBe(word);
  });
});

describe('fakeCompanionTransport', () => {
  it('lists the seeded companions and records each call', async () => {
    const fake = fakeCompanionTransport({ companions: [view()] });
    const list = await fake.list();
    expect(list.map((c) => c.name)).toEqual(['Mochi']);
    expect(fake.calls.map((c) => c.method)).toEqual(['list']);
  });

  it('puts a sleeping companion to idle when woken, and back to sleeping', async () => {
    const fake = fakeCompanionTransport({ companions: [view({ state: 'sleeping' })] });
    expect((await fake.wake('mochi-1')).state).toBe('idle');
    expect((await fake.sleep('mochi-1')).state).toBe('sleeping');
  });

  it('creates a companion with a slug id and the folder name', async () => {
    const fake = fakeCompanionTransport();
    const made = await fake.create(draft);
    expect(made.id).toBe('bun');
    expect(made.folderName).toBe('bun');
    expect(made.state).toBe('starting');
  });

  it('drops a decided pending item', async () => {
    const fake = fakeCompanionTransport({
      memory: { pending: [{ id: 'p1', kind: 'note', text: 'Likes short answers.', flagged: null, targetLine: null }] },
    });
    const after = await fake.decidePending('mochi-1', 'p1', 'drop');
    expect(after.pending).toEqual([]);
  });

  it('throws the plain sentence for a companion that is gone', async () => {
    const fake = fakeCompanionTransport();
    await expect(fake.get('nope')).rejects.toThrow('This companion is not here any more.');
  });
});
