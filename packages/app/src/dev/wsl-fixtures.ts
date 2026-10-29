// Linux Inside for the harness: what the mocked `wsl_*` commands answer (Settings > Agents > Run agents in Linux).
// Example data only: the distros are `Ubuntu-Test` and `Debian-Test`. Without a `wsl` scenario the status says
// "not Windows", so the block stays hidden in every other scenario. Shapes are `lib/wsl.ts`.
import type { AgentPlaceRow, DistroView, WslStatus } from '../lib/wsl';

export interface WslScenario {
  /** What `wsl_status` answers. */
  status: WslStatus;
  /** The Linux-capable agents and where each one runs (`wsl_places_get`). */
  places?: AgentPlaceRow[];
  /** What `wsl_check` answers; `hang` never answers, so the spinner stays. */
  check?: DistroView | 'hang';
}

export const EXAMPLE_PLACES: AgentPlaceRow[] = [
  { agentId: 'claude', label: 'Claude Code', distro: 'Ubuntu-Test', found: null, whyNot: null },
  { agentId: 'codex', label: 'Codex', distro: null, found: null, whyNot: null },
  { agentId: 'opencode', label: 'OpenCode', distro: null, found: null, whyNot: "OpenCode can't run in Linux from Puppet Master yet." },
];

export const EXAMPLE_CHECK: DistroView = {
  name: 'Ubuntu-Test',
  version: 2,
  default: true,
  checkedAtMs: Date.now(),
  git: '2.43',
  interop: true,
  networking: null,
  agents: [
    { agentId: 'claude', label: 'Claude Code', distro: 'Ubuntu-Test', found: true, whyNot: null },
    { agentId: 'codex', label: 'Codex', distro: 'Ubuntu-Test', found: false, whyNot: null },
  ],
};

/** `handled: false` leaves the command to the normal mock. */
export function wslMock(s: WslScenario | undefined, cmd: string, args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('wsl_')) return { handled: false };
  switch (cmd) {
    case 'wsl_status':
      return { handled: true, value: s?.status ?? { state: 'not_windows' } };
    case 'wsl_check':
      return { handled: true, value: s?.check === 'hang' ? new Promise<never>(() => undefined) : (s?.check ?? EXAMPLE_CHECK) };
    case 'wsl_places_get':
      return { handled: true, value: s?.places ?? EXAMPLE_PLACES };
    case 'wsl_place_set': {
      const row = (s?.places ?? EXAMPLE_PLACES).find((r) => r.agentId === args.agentId);
      const base = row ?? { agentId: String(args.agentId), label: String(args.agentId), found: null, whyNot: null };
      return { handled: true, value: { ...base, distro: (args.distro as string | null) ?? null } };
    }
    default:
      return { handled: false };
  }
}
