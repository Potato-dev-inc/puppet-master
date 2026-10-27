// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type CompanionTransport, type CompanionView } from '../lib/companion';
import { COMPANIONS_POLL_MS, useBridgeCompanions, type BridgeCompanions } from './useBridgeCompanions';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const view = (over: Partial<CompanionView> = {}): CompanionView => ({
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'proj',
  state: 'idle',
  stateLine: 'Here. Ask me anything.',
  paneId: 'companion-mochi',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
  ...over,
});

let host: HTMLDivElement;
let root: Root;
let seen: BridgeCompanions;

function Probe({ transport, resyncKey = 0, pushed = null }: { transport: CompanionTransport | null; resyncKey?: number; pushed?: CompanionView[] | null }) {
  seen = useBridgeCompanions(transport, resyncKey, pushed);
  return null;
}

const mount = (props: Parameters<typeof Probe>[0]) =>
  act(async () => {
    root.render(<Probe {...props} />);
  });

const listCalls = (t: { calls: { method: string }[] }) => t.calls.filter((c) => c.method === 'list').length;

function setVisibility(state: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state });
}

beforeEach(() => {
  vi.useFakeTimers();
  setVisibility('visible');
  host = document.createElement('div');
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  // Drop the instance override, so the document reports its own state again.
  delete (document as unknown as { visibilityState?: string }).visibilityState;
  vi.useRealTimers();
});

describe('useBridgeCompanions', () => {
  it('reads the list on start and shows it', async () => {
    const transport = fakeCompanionTransport({ companions: [view()] });
    await mount({ transport });
    expect(seen.companions.map((c) => c.id)).toEqual(['mochi']);
    expect(seen.loaded).toBe(true);
    expect(listCalls(transport)).toBe(1);
  });

  it('reads again every 5 s while the page is visible', async () => {
    const transport = fakeCompanionTransport({ companions: [view()] });
    await mount({ transport });
    await act(async () => {
      vi.advanceTimersByTime(COMPANIONS_POLL_MS);
    });
    expect(listCalls(transport)).toBe(2);
    await act(async () => {
      vi.advanceTimersByTime(2 * COMPANIONS_POLL_MS);
    });
    expect(listCalls(transport)).toBe(4);
  });

  it('does not ask while the page is hidden, and asks again when it comes back', async () => {
    const transport = fakeCompanionTransport({ companions: [view()] });
    await mount({ transport });
    setVisibility('hidden');
    await act(async () => {
      vi.advanceTimersByTime(3 * COMPANIONS_POLL_MS);
    });
    expect(listCalls(transport)).toBe(1);
    setVisibility('visible');
    await act(async () => {
      document.dispatchEvent(new Event('visibilitychange'));
    });
    expect(listCalls(transport)).toBe(2);
  });

  it('a stream resync reads the list again', async () => {
    const transport = fakeCompanionTransport({ companions: [view()] });
    await mount({ transport, resyncKey: 0 });
    await mount({ transport, resyncKey: 1 });
    expect(listCalls(transport)).toBe(2);
  });

  it('puts in the list of a companion event at once', async () => {
    const transport = fakeCompanionTransport({ companions: [view()] });
    await mount({ transport, pushed: null });
    const pushed = [view({ state: 'needs_you', needs: { itemId: 'c1', title: 'Approve?' } })];
    await mount({ transport, pushed });
    expect(seen.companions).toEqual(pushed);
  });

  it('applyView replaces one companion and keeps the others', async () => {
    const transport = fakeCompanionTransport({ companions: [view(), view({ id: 'bit', name: 'Bit', avatar: 'bit' })] });
    await mount({ transport });
    await act(async () => {
      seen.applyView(view({ state: 'sleeping' }));
    });
    expect(seen.companions.map((c) => [c.id, c.state])).toEqual([
      ['mochi', 'sleeping'],
      ['bit', 'idle'],
    ]);
  });

  it('a failed read keeps the list held and still counts as loaded', async () => {
    const transport = fakeCompanionTransport({ companions: [view()] });
    await mount({ transport });
    const down: CompanionTransport = { ...transport, list: () => Promise.reject(new Error('down')) };
    await mount({ transport: down });
    await act(async () => {
      vi.advanceTimersByTime(COMPANIONS_POLL_MS);
    });
    expect(seen.companions.map((c) => c.id)).toEqual(['mochi']);
    expect(seen.loaded).toBe(true);
  });

  it('without a transport nothing is read and the list stays empty', async () => {
    await mount({ transport: null });
    expect(seen.companions).toEqual([]);
    expect(seen.loaded).toBe(false);
  });
});
