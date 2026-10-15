// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from '../../lib/bridge-types';
import type { Digest } from '../../lib/cockpit-api';
import { DigestCard, resetDigestCardForTests } from './DigestCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_800_000_000_000;

type DigestResult = Digest & { empty: boolean; previousSeenMs: number | null };

function digest(overrides: Partial<DigestResult> = {}): DigestResult {
  return {
    sinceMs: NOW - 3_600_000,
    nowMs: NOW,
    agentsWorked: 2,
    turns: 3,
    empty: false,
    previousSeenMs: NOW - 3_600_000,
    lines: [
      { paneId: 'p1', agent: 'claude', turns: 2, sentence: 'Fixed the login redirect.', endedAtMs: NOW, end: 'idle' },
      { paneId: 'p2', agent: 'codex', turns: 1, sentence: 'Added the export button.', endedAtMs: NOW, end: 'idle' },
    ],
    needsYou: [],
    ...overrides,
  };
}

function fakeBridge(result: DigestResult | null = digest()): BridgeClient {
  return {
    digest: vi.fn(async () => {
      if (result === null) throw new Error('offline');
      return result;
    }),
  } as unknown as BridgeClient;
}

let host: HTMLDivElement;
let root: Root;

async function mount(bridge: BridgeClient, bridgeReady = true, onOpen = vi.fn()) {
  await act(async () => {
    root.render(<DigestCard bridge={bridge} bridgeReady={bridgeReady} onOpen={onOpen} />);
  });
  return onOpen;
}

async function unmount() {
  await act(async () => {
    root.unmount();
  });
  root = createRoot(host);
}

const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text);

async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}

function setVisibility(state: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state });
  act(() => {
    document.dispatchEvent(new Event('visibilitychange'));
  });
}

beforeEach(() => {
  resetDigestCardForTests();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('DigestCard', () => {
  it('renders nothing when bridge.digest is missing', async () => {
    await mount({} as BridgeClient);
    expect(host.querySelector('section')).toBeNull();
  });

  it('renders nothing when the digest is empty', async () => {
    const bridge = fakeBridge(digest({ empty: true, lines: [], agentsWorked: 0 }));
    await mount(bridge);
    expect(bridge.digest).toHaveBeenCalledWith({ seen: true });
    expect(host.querySelector('section')).toBeNull();
  });


  it('stays quiet when the last visit was only a minute ago', async () => {
    await mount(fakeBridge(digest({ sinceMs: NOW - 60_000 })));
    expect(host.textContent).toBe('');
  });

  it('does nothing before the bridge is ready', async () => {
    const bridge = fakeBridge();
    await mount(bridge, false);
    expect(bridge.digest).not.toHaveBeenCalled();
    expect(host.querySelector('section')).toBeNull();
  });

  it('shows agents worked, the line sentences and the needs-you count', async () => {
    const bridge = fakeBridge(
      digest({
        needsYou: [
          { paneId: 'p3', agent: 'claude', status: 'waiting_input', prompt: 'Proceed?' },
          { paneId: 'p4', agent: 'codex', status: 'error' },
        ],
      }),
    );
    await mount(bridge);
    expect(bridge.digest).toHaveBeenCalledWith({ seen: true });
    const section = host.querySelector('section');
    expect(section).toBeTruthy();
    expect(section!.getAttribute('aria-label')).toBe('While you were away');
    expect(section!.textContent).toContain('While you were away');
    expect(section!.textContent).toContain('2 agents worked');
    expect(section!.textContent).toContain('Fixed the login redirect.');
    expect(section!.textContent).toContain('Added the export button.');
    expect(section!.textContent).toContain('2 need you');
  });

  it('uses singular wording for one agent and one waiting agent', async () => {
    await mount(
      fakeBridge(digest({ agentsWorked: 1, needsYou: [{ paneId: 'p3', agent: 'claude', status: 'error' }] })),
    );
    expect(host.textContent).toContain('1 agent worked');
    expect(host.textContent).toContain('1 needs you');
  });

  it('caps the rows at three and shows how many are left', async () => {
    const lines = ['a', 'b', 'c', 'd', 'e'].map((id, i) => ({
      paneId: id,
      agent: 'claude',
      turns: 1,
      sentence: `Line ${i}`,
      endedAtMs: NOW,
      end: 'idle',
    }));
    await mount(fakeBridge(digest({ lines })));
    expect(host.querySelectorAll('section ul button')).toHaveLength(3);
    expect(host.textContent).toContain('+2 more');
  });

  it('calls onOpen with the pane id when a line is tapped', async () => {
    const onOpen = await mount(fakeBridge(), true);
    await click(button('Added the export button.'));
    expect(onOpen).toHaveBeenCalledWith('p2');
  });

  it('Got it hides the card, and a remount stays hidden', async () => {
    await mount(fakeBridge());
    expect(host.querySelector('section')).toBeTruthy();
    await click(button('Got it'));
    expect(host.querySelector('section')).toBeNull();

    await unmount();
    const bridge = fakeBridge();
    await mount(bridge);
    expect(bridge.digest).toHaveBeenCalled();
    expect(host.querySelector('section')).toBeNull();
  });

  it('survives a remount before dismissal', async () => {
    await mount(fakeBridge());
    await unmount();
    await mount(fakeBridge(null));
    expect(host.querySelector('section')).toBeTruthy();
    expect(host.textContent).toContain('2 agents worked');
  });

  it('fetches again after 31s hidden, but not after 5s hidden', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(NOW);
    const bridge = fakeBridge();
    await mount(bridge);
    expect(bridge.digest).toHaveBeenCalledTimes(1);

    vi.setSystemTime(NOW + 1_000);
    setVisibility('hidden');
    vi.setSystemTime(NOW + 6_000);
    setVisibility('visible');
    await act(async () => {});
    expect(bridge.digest).toHaveBeenCalledTimes(1);

    vi.setSystemTime(NOW + 10_000);
    setVisibility('hidden');
    vi.setSystemTime(NOW + 41_000);
    setVisibility('visible');
    await act(async () => {});
    expect(bridge.digest).toHaveBeenCalledTimes(2);
    expect(bridge.digest).toHaveBeenLastCalledWith({ seen: true });
  });
});
