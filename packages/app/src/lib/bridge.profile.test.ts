// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { findBridgeUrl } from './bridge';

afterEach(() => {
  invoke.mockReset();
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  vi.unstubAllGlobals();
});

describe('findBridgeUrl with PUPPET_MASTER_PROFILE', () => {
  it("a profile's window uses its own bridge, never the first one that answers", async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue('http://127.0.0.1:17361');
    const fetchSpy = vi.fn();
    vi.stubGlobal('fetch', fetchSpy);
    expect(await findBridgeUrl()).toBe('http://127.0.0.1:17361');
    expect(invoke).toHaveBeenCalledWith('profile_bridge_url');
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it('the default profile (null) scans from 17321 as before', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue(null);
    const fetchSpy = vi.fn(async (url: string) => ({ ok: url.includes(':17321/') }));
    vi.stubGlobal('fetch', fetchSpy);
    expect(await findBridgeUrl()).toBe('http://127.0.0.1:17321');
    expect(String(fetchSpy.mock.calls[0][0])).toContain(':17321/health');
  });
});
