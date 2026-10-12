// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it, vi } from 'vitest';
import { fallbackListing } from '../lib/agentRegistry';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const base = fallbackListing();
const listing = {
  ...base,
  agents: [
    ...base.agents.slice(0, 2),
    { ...base.agents[0], id: 'kiro', label: 'Kiro', source: 'user', experimental: true, modes: { headless: true, visible: false } },
  ],
};
vi.mock('../lib/agentRegistry', async (orig) => ({
  ...(await orig<typeof import('../lib/agentRegistry')>()),
  useAgentRegistry: () => listing,
}));

import { AgentsOverviewCard } from './AgentsOverviewCard';

describe('AgentsOverviewCard', () => {
  it('lists agents with label, source, modes and state, and links to Settings', async () => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    const onOpenSettings = vi.fn();
    await act(async () => {
      createRoot(host).render(<AgentsOverviewCard onOpenSettings={onOpenSettings} />);
    });
    const rows = [...host.querySelectorAll('[data-testid="agent-row"]')];
    expect(rows).toHaveLength(3);
    const kiro = rows[2].textContent ?? '';
    expect(kiro).toContain('Kiro (experimental)');
    expect(kiro).toContain('user - headless');
    expect(kiro).not.toContain('visible');
    expect(kiro).toContain('ready');
    expect(rows[0].textContent).toContain('code - visible + headless');
    await act(async () => {
      host.querySelector('button')?.click();
    });
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });
});
