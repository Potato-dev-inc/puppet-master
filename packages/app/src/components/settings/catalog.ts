// The six sections of Settings and the read-only "Coming later" list.
import type { LaterItem, SectionDef } from './model';
import { agentsRows } from './sections/AgentsSection';
import { bridgeRows } from './sections/BridgeSection';
import { generalRows } from './sections/GeneralSection';
import { keysRows } from './sections/KeysSection';
import { modelsRows } from './sections/ModelsSection';
import { phoneRows } from './sections/PhoneSection';

export const SECTIONS: SectionDef[] = [
  { id: 'general', title: 'General', blurb: 'Project folder, appearance, safety nets and updates.', rows: generalRows },
  { id: 'agents', title: 'Agents', blurb: 'Logins, agents kept ready, and agent definitions.', rows: agentsRows },
  { id: 'models', title: 'Orchestrator & Models', blurb: 'Which brain steers your agents, and which models it can use.', rows: modelsRows },
  { id: 'keys', title: 'Keys', blurb: 'API keys. They are kept in your system keychain when there is one.', rows: keysRows },
  { id: 'bridge', title: 'Bridge & MCP', blurb: 'How other tools and agents reach Puppet Master.', rows: bridgeRows },
  { id: 'phone', title: 'Phone', blurb: 'Pair a phone to watch and steer your agents.', rows: phoneRows },
];

/** Features that are planned but not built. Shown as plain text on the "Coming later" page, never as controls. */
export const LATER_ITEMS: LaterItem[] = [
  { title: 'Desktop notifications', note: 'Choose when Puppet Master interrupts you on this computer.' },
  { title: 'Rules and guardrails', note: 'Limits on what agents may do without asking.' },
  { title: 'Backup and sync', note: 'Keep settings and sessions on more than one computer.' },
  { title: 'Agent marketplace', note: 'Find and install agent definitions made by others.' },
  { title: 'Team workspace', note: 'Share agents and projects with the people you work with.' },
  { title: 'Automation', note: 'Start agents on a schedule or when something happens.' },
  { title: 'Observability', note: 'Traces and logs of what your agents did.' },
  { title: 'Plugins', note: 'Extend Puppet Master with add-ons.' },
  { title: 'Security', note: 'Sign-in and access controls for shared setups.' },
];
