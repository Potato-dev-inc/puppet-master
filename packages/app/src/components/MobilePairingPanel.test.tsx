// @vitest-environment jsdom
// Pairing panel inside Settings > Phone: the URL and port autosave on pause or blur, and Revoke removes one phone.
import { act, type ComponentProps } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEVICES_CHANGED_EVENT, MobilePairingPanel } from './MobilePairingPanel';
import { tauri } from '../lib/tauri';
import { blur, buttonByText, click, hasText, settle, type as typeInto } from './settings/test-utils';

const mocks = vi.hoisted(() => ({
  devices: [] as Array<{ id: string; name: string; public_key: string; paired_at: number }>,
  session: {
    pairing_code: 'ABCD1234',
    expires_at: 4102444800,
    server_public_key: 'pk',
    bridge_url: 'https://pm.example.com/bridge',
    qr_payload: { v: 1, u: 'https://pm.example.com', pk: 'pk', c: 'ABCD1234', e: 4102444800 },
  },
}));

vi.mock('qrcode', () => ({ default: { toCanvas: vi.fn(async () => undefined) } }));
vi.mock('../lib/load-tunnel-dev-info', () => ({ watchTunnelDevInfo: vi.fn(() => () => undefined) }));
vi.mock('../lib/tauri', () => ({
  tauri: {
    createMobilePairingSession: vi.fn(async () => mocks.session),
    listPairedMobileDevices: vi.fn(async () => mocks.devices),
    revokePairedMobileDevice: vi.fn(async () => true),
  },
}));

type Props = ComponentProps<typeof MobilePairingPanel>;

let host: HTMLDivElement;
let root: Root;

async function mount(overrides: Partial<Props> = {}) {
  const props: Props = {
    publicPwaUrl: '',
    devServerPort: 1420,
    onPublicPwaUrlChange: vi.fn(),
    onDevServerPortChange: vi.fn(),
    ...overrides,
  };
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(<MobilePairingPanel {...props} />));
  await settle(5);
  return props;
}

const field = (label: string) => host.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)!;

beforeEach(() => {
  vi.clearAllMocks();
  mocks.devices = [];
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('MobilePairingPanel: autosaving fields', () => {
  it('the public URL is not saved while typing and saves once on blur, as the normalised origin', async () => {
    const props = await mount();
    const url = field('Public PWA URL');
    await typeInto(url, 'pm.example.com/bridge');
    expect(props.onPublicPwaUrlChange).not.toHaveBeenCalled();
    await blur(url);
    expect(props.onPublicPwaUrlChange).toHaveBeenCalledExactlyOnceWith('https://pm.example.com');
  });

  it('the public URL also saves after a short pause in typing', async () => {
    vi.useFakeTimers();
    const props = await mount();
    await typeInto(field('Public PWA URL'), 'https://Pm.Example.com');
    expect(props.onPublicPwaUrlChange).not.toHaveBeenCalled();
    await act(async () => void (await vi.advanceTimersByTimeAsync(700)));
    expect(props.onPublicPwaUrlChange).toHaveBeenCalledExactlyOnceWith('https://pm.example.com');
  });

  it('a port typed on the way to a value does not save; blur saves the clamped port', async () => {
    const props = await mount();
    const port = field('Local dev server port');
    await typeInto(port, '1');
    expect(props.onDevServerPortChange).not.toHaveBeenCalled();
    await typeInto(port, '80');
    await blur(port);
    expect(props.onDevServerPortChange).toHaveBeenCalledExactlyOnceWith(1024);
  });
});

describe('MobilePairingPanel: paired devices and layout', () => {
  it('Revoke removes one phone and reloads the list', async () => {
    mocks.devices = [{ id: 'dev-1', name: 'Pixel 8', public_key: 'k1', paired_at: 1 }];
    await mount();
    expect(hasText(host, 'Pixel 8')).toBe(true);
    await click(buttonByText(host, 'Revoke'));
    await settle();
    expect(tauri.revokePairedMobileDevice).toHaveBeenCalledExactlyOnceWith('dev-1');
    expect(tauri.listPairedMobileDevices).toHaveBeenCalledTimes(2);
  });

  it('reloads the device list when another row (Revoke all devices) changes it', async () => {
    mocks.devices = [{ id: 'dev-1', name: 'Pixel 8', public_key: 'k1', paired_at: 1 }];
    await mount();
    expect(hasText(host, 'Pixel 8')).toBe(true);
    mocks.devices = [];
    await act(async () => void window.dispatchEvent(new Event(DEVICES_CHANGED_EVENT)));
    await settle();
    expect(hasText(host, 'Pixel 8')).toBe(false);
    expect(hasText(host, 'No phones paired yet')).toBe(true);
  });

  it('is a plain settings row that names itself, with no colour classes', async () => {
    await mount();
    expect(host.firstElementChild?.className).toBe('st-row is-block');
    expect(host.querySelector('.st-label span')?.textContent).toBe('Pair a phone');
    expect(host.innerHTML).not.toMatch(/text-pm-(err|amber|accent)|text-(red|emerald|amber)-/);
  });
});
