// @vitest-environment jsdom
// Settings > Agents: the "same file" switch reads the saved value, saves each flip through the command, shows
// "Could not save. Try again." when the save fails, and says it only warns. The API is a fake; no Tauri call happens.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CollisionApi, CollisionSettings as Saved } from '../../../lib/collision';
import { COLLISION_FOOTER, COLLISION_LABEL, COLLISION_SAVE_FAILED, CollisionSettings } from './CollisionSettings';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

function fakeApi(over: Partial<CollisionApi> = {}): CollisionApi & { settingsGet: ReturnType<typeof vi.fn>; settingsSet: ReturnType<typeof vi.fn> } {
  return {
    list: vi.fn(async () => []),
    ignore: vi.fn(async () => undefined),
    settingsGet: vi.fn(async (): Promise<Saved> => ({ enabled: true })),
    settingsSet: vi.fn(async (next: Saved): Promise<Saved> => next),
    onChanged: vi.fn(async () => () => {}),
    ...over,
  } as CollisionApi & { settingsGet: ReturnType<typeof vi.fn>; settingsSet: ReturnType<typeof vi.fn> };
}

const toggle = () => host.querySelector<HTMLButtonElement>('button[role="switch"]');
const alertText = () => host.querySelector('[role="alert"]')?.textContent ?? null;

async function flush() {
  for (let i = 0; i < 4; i++) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

async function mount(api: CollisionApi, onSaved?: () => void) {
  await act(async () => {
    root.render(<CollisionSettings api={api} onSaved={onSaved} />);
  });
  await flush();
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('CollisionSettings', () => {
  it('shows the switch label and the footer, and reads the saved value once', async () => {
    const api = fakeApi();
    await mount(api);
    expect(host.textContent).toContain(COLLISION_LABEL);
    expect(host.textContent).toContain(COLLISION_FOOTER);
    expect(COLLISION_LABEL).toBe('Warn me when two agents change the same file');
    expect(api.settingsGet).toHaveBeenCalledTimes(1);
    expect(toggle()?.getAttribute('aria-checked')).toBe('true');
  });

  it('the footer says the feature only warns and never pauses or stops an agent', async () => {
    await mount(fakeApi());
    expect(COLLISION_FOOTER).toBe(
      'Puppet Master reads the list of changed files, and the changed lines when both agents changed the same file. It never pauses or stops an agent.',
    );
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });

  it('is off when the saved setting is off', async () => {
    const api = fakeApi({ settingsGet: vi.fn(async () => ({ enabled: false })) });
    await mount(api);
    expect(toggle()?.getAttribute('aria-checked')).toBe('false');
    expect(toggle()?.disabled).toBe(false);
  });

  it('turning it off saves { enabled: false } through the command and calls onSaved', async () => {
    const api = fakeApi();
    const onSaved = vi.fn();
    await mount(api, onSaved);
    await act(async () => toggle()?.click());
    await flush();
    expect(api.settingsSet).toHaveBeenCalledWith({ enabled: false });
    expect(toggle()?.getAttribute('aria-checked')).toBe('false');
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('turning it back on saves { enabled: true }', async () => {
    const api = fakeApi({ settingsGet: vi.fn(async () => ({ enabled: false })) });
    await mount(api);
    await act(async () => toggle()?.click());
    await flush();
    expect(api.settingsSet).toHaveBeenLastCalledWith({ enabled: true });
    expect(toggle()?.getAttribute('aria-checked')).toBe('true');
  });

  it('a failed save puts the switch back and shows "Could not save. Try again."', async () => {
    const api = fakeApi({ settingsSet: vi.fn(async () => Promise.reject(new Error('COLLISION_SAVE_FAILED: disk full'))) });
    const onSaved = vi.fn();
    await mount(api, onSaved);
    await act(async () => toggle()?.click());
    await flush();
    expect(COLLISION_SAVE_FAILED).toBe('Could not save. Try again.');
    expect(alertText()).toBe(COLLISION_SAVE_FAILED);
    expect(toggle()?.getAttribute('aria-checked')).toBe('true');
    expect(onSaved).not.toHaveBeenCalled();
  });

  it('clears the error on the next save that works', async () => {
    const settingsSet = vi
      .fn<(next: Saved) => Promise<Saved>>()
      .mockRejectedValueOnce(new Error('COLLISION_SAVE_FAILED'))
      .mockImplementation(async (next) => next);
    await mount(fakeApi({ settingsSet }));
    await act(async () => toggle()?.click());
    await flush();
    expect(alertText()).toBe(COLLISION_SAVE_FAILED);
    await act(async () => toggle()?.click());
    await flush();
    expect(alertText()).toBeNull();
    expect(toggle()?.getAttribute('aria-checked')).toBe('false');
  });

  it('is disabled and never saves when the desktop shell cannot answer', async () => {
    const api = fakeApi({ settingsGet: vi.fn(async () => Promise.reject(new Error('Desktop shell required: collision_settings_get'))) });
    await mount(api);
    expect(toggle()?.disabled).toBe(true);
    await act(async () => toggle()?.click());
    await flush();
    expect(api.settingsSet).not.toHaveBeenCalled();
  });
});
