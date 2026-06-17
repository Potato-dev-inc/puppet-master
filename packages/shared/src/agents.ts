import { z } from 'zod';

export const AgentTypeSchema = z.enum(['claude', 'codex', 'opencode', 'powershell', 'bash', 'cursor']);
export type AgentType = z.infer<typeof AgentTypeSchema>;

export interface AgentPreset {
  type: AgentType;
  label: string;
  description: string;
  command: string;
  baseArgs: string[];
  isTui: boolean;
  icon: string;
}

export const AGENT_PRESETS: Record<AgentType, AgentPreset> = {
  claude: {
    type: 'claude',
    label: 'Claude Code',
    description: 'Anthropic Claude Code CLI',
    command: 'claude.exe',
    baseArgs: [],
    isTui: true,
    icon: 'CC',
  },
  codex: {
    type: 'codex',
    label: 'Codex CLI',
    description: 'OpenAI Codex CLI',
    command: 'codex.exe',
  // Puppet Master: workspace sandbox, skip approval UI (permissions handled by autopilot if needed)
    baseArgs: ['--sandbox', 'workspace-write', '--ask-for-approval', 'never'],
    isTui: true,
    icon: 'CX',
  },
  opencode: {
    type: 'opencode',
    label: 'OpenCode',
    description: 'OpenCode CLI',
    command: 'opencode.cmd',
    baseArgs: [],
    isTui: true,
    icon: 'OC',
  },
  powershell: {
    type: 'powershell',
    label: 'PowerShell',
    description: 'Windows PowerShell',
    command: 'powershell.exe',
    baseArgs: ['-NoLogo'],
    isTui: true,
    icon: 'PS',
  },
  bash: {
    type: 'bash',
    label: 'Bash',
    description: 'Git Bash / WSL bash',
    command: 'bash.exe',
    baseArgs: ['--login'],
    isTui: true,
    icon: 'Bash',
  },
  cursor: {
    type: 'cursor',
    label: 'Cursor IDE',
    description: 'Opens project in Cursor (not an agent TUI)',
    command: 'cursor.cmd',
    baseArgs: [],
    isTui: false,
    icon: 'ID',
  },
};

export function getPreset(type: AgentType): AgentPreset {
  return AGENT_PRESETS[type];
}

export function listPresets(): AgentPreset[] {
  return Object.values(AGENT_PRESETS);
}