// @vitest-environment jsdom
// The companion list: read once, kept current by the `companion://changed` event, null when there is no backend.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type CompanionTransport, type CompanionView } from '../lib/companion';
import { useCompanionMemory, useCompanions, type CompanionsState } from './useCompanions';

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
  vi.restoreAllMocks();
});

const view = (id: string, name: string): CompanionView => ({
  id,
  name,
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'home',
  state: 'idle',
  stateLine: 'Here. Ask me anything.',
  paneId: `companion-${id}`,
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
});

/** A transport with the event: `emit` plays a `companion://changed` frame. */
function withEvent(base: CompanionTransport) {
  let emit: ((list: CompanionView[]) => void) | null = null;
  const unlisten = vi.fn();
  const transport: CompanionTransport = {
    ...base,
    onChanged: (cb) => {
      emit = cb;
      return Promise.resolve(unlisten);
    },
  };
  return { transport, unlisten, emit: (list: CompanionView[]) => emit?.(list) };
}

let latest: CompanionsState | null = null;
function Probe({ transport }: { transport: CompanionTransport }) {
  latest = useCompanions(transport);
  return null;
}

describe('useCompanions', () => {
  it('reads the list once, then follows the changed event', async () => {
    const { transport, emit } = withEvent(fakeCompanionTransport({ companions: [view('mochi', 'Mochi')] }));
    await act(async () => root.render(<Probe transport={transport} />));
    await act(async () => {});
    expect(latest?.companions?.map((c) => c.name)).toEqual(['Mochi']);

    await act(async () => emit([view('mochi', 'Mochi'), view('bit', 'Bit')]));
    expect(latest?.companions?.map((c) => c.name)).toEqual(['Mochi', 'Bit']);
  });

  it('is null where there is no companion backend, so no rows show', async () => {
    const base = fakeCompanionTransport();
    base.list = () => Promise.reject(new Error('Desktop shell required: companion_list'));
    await act(async () => root.render(<Probe transport={base} />));
    await act(async () => {});
    expect(latest?.companions).toBeNull();
  });

  it('stops listening when it leaves the screen', async () => {
    const { transport, unlisten } = withEvent(fakeCompanionTransport());
    await act(async () => root.render(<Probe transport={transport} />));
    await act(async () => {});
    await act(async () => root.unmount());
    root = createRoot(host);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});

function MemoryProbe({ transport, id }: { transport: CompanionTransport; id: string }) {
  const { memory } = useCompanionMemory(transport, id, 'idle|0');
  return <p>{memory ? memory.soul : 'loading'}</p>;
}

describe('useCompanionMemory', () => {
  it("reads the companion's memory files", async () => {
    const transport = fakeCompanionTransport({ memory: { soul: 'Warm.', user: '', memory: '', pending: [] } });
    await act(async () => root.render(<MemoryProbe transport={transport} id="mochi" />));
    await act(async () => {});
    expect(host.textContent).toBe('Warm.');
  });
});
