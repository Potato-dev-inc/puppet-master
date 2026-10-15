// The six sections of Settings and the read-only "Coming later" list.
//
// `BASE_SECTIONS` is what each section file defines. `SECTIONS` is what the window shows: the same rows,
// placed and grouped by `arrange.tsx` (group titles, order, General > Labs, links where rows moved).
import { arrangeSections } from './arrange';
import type { LaterItem, SectionDef } from './model';
import { agentsRows } from './sections/AgentsSection';
import { bridgeRows } from './sections/BridgeSection';
import { generalRows } from './sections/GeneralSection';
import { keysRows } from './sections/KeysSection';
import { modelsRows } from './sections/ModelsSection';
import { phoneRows } from './sections/PhoneSection';

export const BASE_SECTIONS: SectionDef[] = [
  { id: 'general', title: 'General', blurb: 'Project, appearance, safety, early features and updates.', rows: generalRows },
  { id: 'agents', title: 'Agents', blurb: 'Logins, keeping agents ready, versions and your own agents.', rows: agentsRows },
  { id: 'models', title: 'Orchestrator & Models', blurb: 'Choose the AI that coordinates your other agents, and which models it can use.', rows: modelsRows },
  { id: 'keys', title: 'Keys', blurb: 'API keys. They are kept in your system keychain when there is one.', rows: keysRows },
  { id: 'bridge', title: 'Bridge & MCP', blurb: 'How other tools and agents connect to Puppet Master.', rows: bridgeRows },
  { id: 'phone', title: 'Phone', blurb: 'Pair a phone to watch and steer your agents.', rows: phoneRows },
];

export const SECTIONS: SectionDef[] = arrangeSections(BASE_SECTIONS);

/** Features that are planned but not built. Shown as plain text on the "Coming later" page, never as controls. */
export const LATER_ITEMS: LaterItem[] = [
  { title: 'Desktop notifications', note: 'Choose when Puppet Master interrupts you on this computer.' },
  { title: 'Rules and guardrails', note: 'Limits on what agents may do without asking.' },
  { title: 'Backup and sync', note: 'Keep settings and sessions on more than one computer.' },
  { title: 'Agent marketplace', note: 'Find and install agent definitions made by others.' },
  { title: 'Automation', note: 'Start agents on a schedule or when something happens.' },
  { title: 'Observability', note: 'Traces and logs of what your agents did.' },
];
