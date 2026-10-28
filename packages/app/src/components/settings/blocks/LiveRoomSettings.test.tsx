// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveRoomApi, LiveRoomConfig } from '../../../lib/live-room';
import { LiveRoomSettings } from './LiveRoomSettings';

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
});

const DEFAULTS: LiveRoomConfig = { relay: '', minutes: 60, maxViewers: 10 };

function fakeApi(config: LiveRoomConfig = DEFAULTS) {
  return {
    configGet: vi.fn(async () => config),
    configSet: vi.fn(async (_cfg: LiveRoomConfig) => {}),
  };
}

async function mount(api: ReturnType<typeof fakeApi>, teamRelay = '') {
  const onSaved = vi.fn();
  await act(async () => {
    root.render(<LiveRoomSettings api={api as unknown as LiveRoomApi} teamRelay={teamRelay} onSaved={onSaved} />);
  });
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
  return { onSaved };
}

const field = (label: string) => host.querySelector(`[aria-label="${label}"]`) as HTMLInputElement & HTMLSelectElement;

/** Types into a React-controlled input, the way a person would. */
async function type(el: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function leave(el: HTMLInputElement) {
  await act(async () => {
    el.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
  });
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

describe('LiveRoomSettings', () => {
  it('shows the saved relay', async () => {
    await mount(fakeApi({ ...DEFAULTS, relay: 'https://relay.example.test' }));
    expect(field('Relay address').value).toBe('https://relay.example.test');
  });

  it('shows the team relay as the placeholder when a team relay is set', async () => {
    await mount(fakeApi(), 'https://team.example.test');
    expect(field('Relay address').placeholder).toBe('Uses your team relay');
  });

  it('saves the relay when the field loses focus, keeping the other settings', async () => {
    const api = fakeApi();
    const { onSaved } = await mount(api);
    await type(field('Relay address'), '  https://relay.example.test  ');
    await leave(field('Relay address'));
    expect(api.configSet).toHaveBeenCalledWith({ relay: 'https://relay.example.test', minutes: 60, maxViewers: 10 });
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('shows the refusal for an http:// remote relay as it is', async () => {
    const api = fakeApi();
    api.configSet.mockRejectedValue(new Error('Use https:// for a relay on another computer.'));
    await mount(api);
    await type(field('Relay address'), 'http://relay.example.test');
    await leave(field('Relay address'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Use https:// for a relay on another computer.');
  });

  it('saves the default length when it changes', async () => {
    const api = fakeApi();
    await mount(api);
    await act(async () => {
      const select = field('Default length');
      select.value = '120';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(api.configSet).toHaveBeenCalledWith({ relay: '', minutes: 120, maxViewers: 10 });
  });

  it('offers the four viewer choices', async () => {
    await mount(fakeApi());
    const options = Array.from(field('Default viewers').options).map((o) => o.value);
    expect(options).toEqual(['5', '10', '25', '50']);
  });

  it('keeps the one-line privacy note', async () => {
    await mount(fakeApi());
    expect(host.textContent).toContain('Rooms are end-to-end encrypted. The relay sees only sizes and times.');
  });
});
