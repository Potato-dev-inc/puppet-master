// @vitest-environment jsdom
// Settings > Phone > Devices: one switch per ability, the expiry line, Revoke, and the list follows
// changes made elsewhere.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PairedDeviceInfo } from '@puppet-master/shared';
import { PairedDevices } from './PairedDevices';
import { DEVICES_CHANGED_EVENT } from '../../MobilePairingPanel';
import { tauri } from '../../../lib/tauri';
import { buttonByText, click, hasText, settle } from '../test-utils';

const DAY = 24 * 60 * 60;
const mocks = vi.hoisted(() => ({ devices: [] as unknown[] }));

vi.mock('../../../lib/tauri', () => ({
  tauri: {
    listPairedMobileDevices: vi.fn(async () => mocks.devices),
    revokePairedMobileDevice: vi.fn(async () => true),
    setPairedMobileDeviceCaps: vi.fn(async () => true),
  },
}));

function device(over: Partial<PairedDeviceInfo>): PairedDeviceInfo {
  return {
    id: '00000000-0000-4000-8000-000000000001',
    name: 'Pixel 8',
    public_key: 'k',
    paired_at: 1,
    caps: ['watch', 'answer', 'type', 'control'],
    expires_at: 2_000_000_000,
    last_seen: 1,
    expired: false,
    idle_expiry_secs: 30 * DAY,
    ...over,
  };
}

let host: HTMLDivElement;
let root: Root;

async function mount() {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(<PairedDevices />));
  await settle(5);
}

const sw = (label: string) => host.querySelector<HTMLButtonElement>(`[role="switch"][aria-label="${label}"]`);

beforeEach(() => {
  vi.clearAllMocks();
  mocks.devices = [];
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('PairedDevices', () => {
  it('shows each ability as a switch and the sliding expiry in plain words', async () => {
    mocks.devices = [device({ caps: ['watch', 'answer'] })];
    await mount();
    expect(hasText(host, 'Pixel 8')).toBe(true);
    expect(hasText(host, 'Active · expires if unused for 30 days')).toBe(true);
    expect(sw('Pixel 8: Watch')?.getAttribute('aria-checked')).toBe('true');
    expect(sw('Pixel 8: Answer')?.getAttribute('aria-checked')).toBe('true');
    expect(sw('Pixel 8: Use the terminal')?.getAttribute('aria-checked')).toBe('false');
    expect(sw('Pixel 8: Start tasks')?.getAttribute('aria-checked')).toBe('false');
    // no raw ids in the chrome
    expect(host.textContent).not.toContain('00000000');
  });

  it('flipping a switch saves the new set of abilities for that phone', async () => {
    mocks.devices = [device({})];
    await mount();
    await click(sw('Pixel 8: Use the terminal'));
    await settle();
    expect(tauri.setPairedMobileDeviceCaps).toHaveBeenCalledExactlyOnceWith(
      '00000000-0000-4000-8000-000000000001',
      ['watch', 'answer', 'control'],
    );
  });

  it('a phone paired by an older app (no caps stored) shows every ability on', async () => {
    const old = device({});
    delete old.caps;
    mocks.devices = [old];
    await mount();
    for (const label of ['Watch', 'Answer', 'Use the terminal', 'Start tasks']) {
      expect(sw(`Pixel 8: ${label}`)?.getAttribute('aria-checked')).toBe('true');
    }
  });

  it('an expired phone says it was signed out, has no switches and can be removed', async () => {
    mocks.devices = [device({ expired: true })];
    await mount();
    expect(hasText(host, 'Signed out after 30 days without use')).toBe(true);
    expect(sw('Pixel 8: Watch')).toBeNull();
    await click(buttonByText(host, 'Remove'));
    await settle();
    expect(tauri.revokePairedMobileDevice).toHaveBeenCalledOnce();
  });

  it('Revoke signs one phone out and reloads the list', async () => {
    mocks.devices = [device({})];
    await mount();
    mocks.devices = [];
    await click(buttonByText(host, 'Revoke'));
    await settle();
    expect(tauri.revokePairedMobileDevice).toHaveBeenCalledExactlyOnceWith('00000000-0000-4000-8000-000000000001');
    expect(hasText(host, 'No phones paired yet')).toBe(true);
  });

  it('reloads when devices change elsewhere (a new pairing, Revoke all)', async () => {
    await mount();
    expect(hasText(host, 'No phones paired yet')).toBe(true);
    mocks.devices = [device({ name: 'New phone' })];
    await act(async () => void window.dispatchEvent(new Event(DEVICES_CHANGED_EVENT)));
    await settle();
    expect(hasText(host, 'New phone')).toBe(true);
  });
});
