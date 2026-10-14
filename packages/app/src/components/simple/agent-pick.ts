// Which agent Simple mode starts: the first one that can run a visible task and is on this
// computer, in the registry's own order. Generic code: it reads the capabilities each agent
// declares, never an agent's name.
import type { AgentDescriptor, AgentRegistryListing } from '../../lib/agentRegistry';

export function usableAgent(a: AgentDescriptor): boolean {
  return a.dispatchable && a.installed && a.kind === 'cli' && a.modes.visible;
}

export function pickAgent(listing: AgentRegistryListing | null | undefined): AgentDescriptor | null {
  return listing?.agents.find(usableAgent) ?? null;
}
