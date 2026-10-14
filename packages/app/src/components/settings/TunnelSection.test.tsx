// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TunnelStatus } from '../../lib/tauri-types';
import { TunnelSection, type TunnelApi } from './TunnelSection';
import { draftFromConfig, draftProblem, statusLine, toInput } from './tunnel-model';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const quick: TunnelStatus = {
  mode: 'quick',
  public_url: 'https://abc.trycloudflare.com',
  state: 'running',
  error: null,
  config: { mode: 'quick', hostname: null, url: null, has_token: false },
};
const named: TunnelStatus = {
  mode: 'named',
  public_url: 'https://pm.example.com',
  state: 'running',
  error: null,
  config: { mode: 'named', hostname: 'pm.example.com', url: null, has_token: true },
};

describe('tunnel model', () => {
  it('requires a token for a new named tunnel but not when one is stored', () => {
    const draft = { ...draftFromConfig(quick.config), mode: 'named' as const, hostname: 'pm.example.com' };
    expect(draftProblem(draft, quick.config)).toMatch(/token/);
    expect(draftProblem({ ...draft, token: 'T' }, quick.config)).toBeNull();
    expect(draftProblem(draft, named.config)).toBeNull();
  });

  it('sends token null (keep) when the field is blank, and trims input', () => {
    expect(toInput({ mode: 'named', hostname: ' pm.example.com ', token: '', url: '' })).toEqual({
      mode: 'named',
      hostname: 'pm.example.com',
      token: null,
    });
    expect(toInput({ mode: 'custom', hostname: '', token: '', url: ' https://x.dev ' })).toEqual({ mode: 'custom', url: 'https://x.dev' });
  });

  it('describes the states in plain words', () => {
    expect(statusLine(null).text).toMatch(/desktop app/);
    expect(statusLine({ ...named, state: 'starting' }).text).toBe('Starting…');
    expect(statusLine({ ...named, state: 'error', error: 'Cloudflare rejected the tunnel token.' }).tone).toBe('bad');
    expect(statusLine({ ...quick, mode: 'env' }).text).toContain('set outside the app');
  });
});

describe('TunnelSection', () => {
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

  const flush = () => act(async () => { await Promise.resolve(); });
  const setValue = (el: HTMLInputElement | HTMLSelectElement, value: string) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  };
  const button = (label: string) => [...host.querySelectorAll('button')].find((b) => b.textContent?.includes(label))!;

  function mockApi(initial: TunnelStatus): TunnelApi & { save: ReturnType<typeof vi.fn> } {
    return {
      getStatus: vi.fn(async () => initial),
      save: vi.fn(async () => named),
      restart: vi.fn(async () => initial),
      test: vi.fn(async () => ({ ok: true, url: 'u', http_status: 200, latency_ms: 42, message: 'Connected in 42 ms.' })),
    };
  }

  it('saves a named tunnel through the api and never renders a stored token', async () => {
    const api = mockApi(quick);
    await act(async () => root.render(<TunnelSection api={api} />));
    await flush();
    expect(host.querySelector('[data-testid="tunnel-url"]')?.textContent).toBe('https://abc.trycloudflare.com');
    expect(button('Save and restart').disabled).toBe(true);

    act(() => setValue(host.querySelector('select')!, 'named'));
    act(() => setValue(host.querySelector('input[aria-label="Tunnel hostname"]')!, 'pm.example.com'));
    expect(button('Save and restart').disabled).toBe(true); // token still missing
    act(() => setValue(host.querySelector('input[aria-label="Tunnel token"]')!, 'SECRET-TOKEN'));
    expect(button('Save and restart').disabled).toBe(false);
    await act(async () => button('Save and restart').click());
    expect(api.save).toHaveBeenCalledWith({ mode: 'named', hostname: 'pm.example.com', token: 'SECRET-TOKEN' });
    await flush();
    expect(host.textContent).not.toContain('SECRET-TOKEN');
    expect((host.querySelector('input[aria-label="Tunnel token"]') as HTMLInputElement).value).toBe('');
    expect(host.querySelector('[data-testid="tunnel-url"]')?.textContent).toBe('https://pm.example.com');
  });

  it('runs test connection and shows the result', async () => {
    const api = mockApi(named);
    await act(async () => root.render(<TunnelSection api={api} />));
    await flush();
    await act(async () => button('Test connection').click());
    expect(host.querySelector('[data-testid="tunnel-test"]')?.textContent).toContain('Reachable. Connected in 42 ms.');
  });
});
