import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  FALLBACK_AGENT_IDS,
  fallbackListing,
  listAgentRegistry,
  resetAgentRegistryCache,
} from './agentRegistry';

const NINE = [
  'claude',
  'codex',
  'opencode',
  'opencode_native',
  'cursor_agent',
  'cursor',
  'cmd',
  'powershell',
  'bash',
];

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

describe('agentRegistry', () => {
  beforeEach(() => {
    invoke.mockReset();
    resetAgentRegistryCache();
  });
  afterEach(() => setTauri(false));

  it('compiled fallback lists the nine code agents in registry order', () => {
    expect(FALLBACK_AGENT_IDS).toEqual(NINE);
    const listing = fallbackListing();
    expect(listing.source).toBe('fallback');
    expect(listing.agents.map((agent) => agent.id)).toEqual(NINE);
    expect(listing.dispatchable_ids).toEqual(['claude', 'codex', 'opencode', 'opencode_native', 'cursor_agent']);
    expect(listing.agents.find((agent) => agent.id === 'claude')?.label).toBe('Claude Code');
  });

  it('returns the compiled nine when the command rejects', async () => {
    setTauri(true);
    invoke.mockRejectedValue(new Error('command agents_list not found'));
    const listing = await listAgentRegistry();
    expect(invoke).toHaveBeenCalledWith('agents_list', undefined);
    expect(listing.source).toBe('fallback');
    expect(listing.agents.map((agent) => agent.id)).toEqual(NINE);
  });

  it('returns the compiled nine outside the desktop shell without invoking', async () => {
    const listing = await listAgentRegistry();
    expect(invoke).not.toHaveBeenCalled();
    expect(listing.source).toBe('fallback');
    expect(listing.agents).toHaveLength(9);
  });

  it('uses the registry listing when the command answers, and caches it', async () => {
    setTauri(true);
    const kilo = { ...fallbackListing().agents[0], id: 'kilo', label: 'Kilo Code', source: 'user' };
    invoke.mockResolvedValue({ agents: [kilo], dispatchable_ids: ['kilo'] });
    const listing = await listAgentRegistry();
    expect(listing.source).toBe('registry');
    expect(listing.agents.map((agent) => agent.id)).toEqual(['kilo']);
    await listAgentRegistry();
    expect(invoke).toHaveBeenCalledTimes(1);
    await listAgentRegistry({ refresh: true });
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('falls back when the command answers with a malformed body', async () => {
    setTauri(true);
    invoke.mockResolvedValue({ nope: true });
    expect((await listAgentRegistry()).source).toBe('fallback');
  });
});
