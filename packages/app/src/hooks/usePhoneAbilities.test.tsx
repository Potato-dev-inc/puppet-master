// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from '../lib/bridge-types';
import type { PhoneAbilities } from '../lib/phone-abilities';
import { usePhoneAbilities } from './usePhoneAbilities';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let seen: PhoneAbilities;

function Probe({ bridge, rev }: { bridge: BridgeClient | null; rev: number }) {
  seen = usePhoneAbilities(bridge, rev);
  return null;
}

const bridgeWith = (pairMe?: BridgeClient['pairMe']) => ({ baseUrl: 'http://x', pairMe }) as unknown as BridgeClient;
const mount = (bridge: BridgeClient | null, rev = 0) =>
  act(async () => {
    root.render(<Probe bridge={bridge} rev={rev} />);
  });

beforeEach(() => {
  host = document.createElement('div');
  root = createRoot(host);
});
afterEach(() => act(() => root.unmount()));

describe('usePhoneAbilities', () => {
  it('shows everything until the PC answers, then hides typing for a phone without it', async () => {
    const pairMe = vi.fn(async () => ({ local: false, caps: ['watch', 'answer'] }));
    await mount(bridgeWith(pairMe));
    expect(pairMe).toHaveBeenCalledTimes(1);
    expect(seen.canType).toBe(false);
    expect(seen.typingNote).toBe('This phone can watch and answer, not type.');
  });

  it('keeps the input box when the PC cannot say (old build, dropped link)', async () => {
    await mount(bridgeWith(vi.fn(async () => Promise.reject(new Error('404')))));
    expect(seen.canType).toBe(true);
    expect(seen.typingNote).toBeNull();
    await mount(bridgeWith(undefined));
    expect(seen.canType).toBe(true);
  });

  it('asks again when the event stream resyncs, so a change made on the PC shows up', async () => {
    const pairMe = vi
      .fn()
      .mockResolvedValueOnce({ local: false, caps: ['watch', 'answer'] })
      .mockResolvedValueOnce({ local: false, caps: ['watch', 'answer', 'type'] });
    const bridge = bridgeWith(pairMe);
    await mount(bridge, 0);
    expect(seen.canType).toBe(false);
    await mount(bridge, 1);
    expect(seen.canType).toBe(true);
  });
});
