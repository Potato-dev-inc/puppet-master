// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  FALLBACK_AGENT_IDS,
  fallbackListing,
  isTuiAgentId,
  launchMenuEntries,
  paneMenuEntries,
  openableLaunchEntries,
  agentReadyMessage,
  HEADLESS_ONLY_REASON,
  handleDefinitionsChanged,
  listAgentRegistry,
  onAgentDefinitionsChanged,
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

describe('spawn menu entries (launchMenuEntries)', () => {
  beforeEach(() => resetAgentRegistryCache());

  it('renders the unchanged launch entries from the compiled fallback (nine agents registered)', async () => {
    const listing = fallbackListing();
    expect(listing.agents).toHaveLength(9);
    const entries = launchMenuEntries(listing);
    expect(entries.map((e) => e.type)).toEqual(['opencode_native', 'powershell']);
    expect(entries.map((e) => e.label)).toEqual(['OpenCode (API)', 'PowerShell']);
  });

  it('lists visible agents clickable and headless-only agents disabled with a reason; marks experimental', () => {
    const base = fallbackListing();
    const kilo = { ...base.agents[0], id: 'kilo', label: 'Kilo Code', source: 'declarative' };
    const kiro = { ...base.agents[0], id: 'kiro', label: 'Kiro', source: 'user', modes: { headless: true, visible: false } };
    const gen = { ...base.agents[0], id: 'mycoder', label: 'mycoder', source: 'user', experimental: true };
    const entries = launchMenuEntries({ ...base, agents: [...base.agents, kilo, kiro, gen], source: 'registry' });
    expect(entries.map((e) => e.type)).toEqual(['opencode_native', 'powershell', 'kilo', 'kiro', 'mycoder']);
    expect(entries[2].disabled).toBeUndefined();
    expect(entries[3]).toMatchObject({ label: 'Kiro', disabled: true, disabledReason: HEADLESS_ONLY_REASON });
    expect(HEADLESS_ONLY_REASON).toMatch(/Headless only/);
    expect(entries[4].experimental).toBe(true);
    expect(openableLaunchEntries(entries).map((e) => e.type)).toEqual(['opencode_native', 'powershell', 'kilo', 'mycoder']);
  });

  it('paneMenuEntries lists every built-in code agent like the tray, headless-only ones disabled', () => {
    const base = fallbackListing();
    expect(base.agents.every((a) => a.source === 'code')).toBe(true);
    const kilo = { ...base.agents[0], id: 'kilo', label: 'Kilo Code', source: 'declarative' };
    const kiro = { ...base.agents[0], id: 'kiro', label: 'Kiro', source: 'user', modes: { headless: true, visible: false } };
    const hidden = { ...base.agents[0], id: 'ghost', label: 'Ghost', source: 'user', modes: { headless: false, visible: false } };
    const entries = paneMenuEntries({ ...base, agents: [...base.agents, kilo, kiro, hidden], source: 'registry' });
    expect(entries.map((e) => e.type)).toEqual([...base.agents.map((a) => a.id), 'kilo', 'kiro']);
    expect(entries.find((e) => e.type === 'claude')).toMatchObject({ label: expect.any(String) });
    expect(entries.find((e) => e.type === 'claude')?.disabled).toBeUndefined();
    expect(entries.find((e) => e.type === 'kiro')).toMatchObject({ disabled: true, disabledReason: HEADLESS_ONLY_REASON });
  });

  it('agentReadyMessage says where the approved agent shows up, never asks for a restart', () => {
    const base = fallbackListing();
    const kiro = { ...base.agents[0], id: 'kiro', label: 'Kiro', source: 'user', modes: { headless: true, visible: false } };
    const listing = { ...base, agents: [...base.agents, kiro] };
    expect(agentReadyMessage(listing, 'kiro')).toBe("Kiro is ready - it's in the New session menu (headless only).");
    expect(agentReadyMessage(listing, 'claude')).toBe(`${base.agents[0].label} is ready - it's in the New session menu.`);
    expect(agentReadyMessage(listing, 'ghost')).not.toMatch(/restart Puppet/i);
  });

  it('isTuiAgentId keeps the compiled answers and treats registered cli agents as TUIs', async () => {
    expect(isTuiAgentId('claude')).toBe(true);
    expect(isTuiAgentId('cursor_agent')).toBe(false);
    expect(isTuiAgentId('powershell')).toBe(false);
    expect(isTuiAgentId('nope')).toBe(false);
    setTauri(true);
    const base = fallbackListing();
    invoke.mockResolvedValue({
      agents: [...base.agents, { ...base.agents[0], id: 'kilo', kind: 'cli', source: 'declarative' }],
      dispatchable_ids: ['kilo'],
    });
    await listAgentRegistry();
    await Promise.resolve();
    expect(isTuiAgentId('kilo')).toBe(true);
    setTauri(false);
  });
  it('a definitions change refetches the listing and notifies subscribers', async () => {
    setTauri(true);
    const body = (ids: string[]) => ({
      agents: ids.map((id) => ({ id })),
      dispatchable_ids: ids,
    });
    invoke.mockResolvedValueOnce(body(['claude']));
    expect((await listAgentRegistry()).agents.map((a) => a.id)).toEqual(['claude']);
    const seen = vi.fn();
    const off = onAgentDefinitionsChanged(seen);
    invoke.mockResolvedValueOnce(body(['claude', 'kilo']));
    await handleDefinitionsChanged();
    expect(seen).toHaveBeenCalledTimes(1);
    expect((await listAgentRegistry()).agents.map((a) => a.id)).toEqual(['claude', 'kilo']);
    off();
    invoke.mockResolvedValueOnce(body(['claude']));
    await handleDefinitionsChanged();
    expect(seen).toHaveBeenCalledTimes(1);
  });
});
