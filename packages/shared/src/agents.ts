import { z } from 'zod';
import { detectPlatform, type PuppetPlatform } from './platform.js';

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

interface AgentCommandSpec {
  command: string;
  baseArgs: string[];
  description: string;
}

const AGENT_META: Record<
  AgentType,
  Pick<AgentPreset, 'type' | 'label' | 'isTui' | 'icon'>
> = {
  claude: {
    type: 'claude',
    label: 'Claude Code',
    isTui: true,
    icon: 'CC',
  },
  codex: {
    type: 'codex',
    label: 'Codex CLI',
    isTui: true,
    icon: 'CX',
  },
  opencode: {
    type: 'opencode',
    label: 'OpenCode',
    isTui: true,
    icon: 'OC',
  },
  powershell: {
    type: 'powershell',
    label: 'PowerShell',
    isTui: true,
    icon: 'PS',
  },
  bash: {
    type: 'bash',
    label: 'Shell',
    isTui: true,
    icon: 'Bash',
  },
  cursor: {
    type: 'cursor',
    label: 'Cursor IDE',
    isTui: false,
    icon: 'ID',
  },
};

const CODEX_ARGS = ['--sandbox', 'workspace-write', '--ask-for-approval', 'never'];

const PLATFORM_COMMANDS: Record<PuppetPlatform, Record<AgentType, AgentCommandSpec>> = {
  windows: {
    claude: {
      command: 'claude.exe',
      baseArgs: [],
      description: 'Anthropic Claude Code CLI',
    },
    codex: {
      command: 'codex.exe',
      baseArgs: CODEX_ARGS,
      description: 'OpenAI Codex CLI',
    },
    opencode: {
      command: 'opencode.cmd',
      baseArgs: [],
      description: 'OpenCode CLI',
    },
    powershell: {
      command: 'powershell.exe',
      baseArgs: ['-NoLogo'],
      description: 'Windows PowerShell',
    },
    bash: {
      command: 'bash.exe',
      baseArgs: ['--login'],
      description: 'Git Bash / WSL bash',
    },
    cursor: {
      command: 'cursor.cmd',
      baseArgs: [],
      description: 'Opens project in Cursor (not an agent TUI)',
    },
  },
  macos: {
    claude: {
      command: 'claude',
      baseArgs: [],
      description: 'Anthropic Claude Code CLI',
    },
    codex: {
      command: 'codex',
      baseArgs: CODEX_ARGS,
      description: 'OpenAI Codex CLI',
    },
    opencode: {
      command: 'opencode',
      baseArgs: [],
      description: 'OpenCode CLI',
    },
    powershell: {
      command: 'pwsh',
      baseArgs: ['-NoLogo'],
      description: 'PowerShell (pwsh)',
    },
    bash: {
      command: 'zsh',
      baseArgs: ['-l'],
      description: 'macOS Zsh (default shell)',
    },
    cursor: {
      command: 'cursor',
      baseArgs: [],
      description: 'Opens project in Cursor (not an agent TUI)',
    },
  },
  linux: {
    claude: {
      command: 'claude',
      baseArgs: [],
      description: 'Anthropic Claude Code CLI',
    },
    codex: {
      command: 'codex',
      baseArgs: CODEX_ARGS,
      description: 'OpenAI Codex CLI',
    },
    opencode: {
      command: 'opencode',
      baseArgs: [],
      description: 'OpenCode CLI',
    },
    powershell: {
      command: 'pwsh',
      baseArgs: ['-NoLogo'],
      description: 'PowerShell (pwsh)',
    },
    bash: {
      command: 'bash',
      baseArgs: ['--login'],
      description: 'Bash shell',
    },
    cursor: {
      command: 'cursor',
      baseArgs: [],
      description: 'Opens project in Cursor (not an agent TUI)',
    },
  },
};

function buildPreset(type: AgentType, platform: PuppetPlatform): AgentPreset {
  const meta = AGENT_META[type];
  const spec = PLATFORM_COMMANDS[platform][type];
  return {
    ...meta,
    description: spec.description,
    command: spec.command,
    baseArgs: spec.baseArgs,
  };
}

export function getPreset(type: AgentType, platform?: PuppetPlatform): AgentPreset {
  return buildPreset(type, platform ?? detectPlatform());
}

export function listPresets(platform?: PuppetPlatform): AgentPreset[] {
  const resolved = platform ?? detectPlatform();
  return AgentTypeSchema.options.map((type) => buildPreset(type, resolved));
}

export function getAgentPresets(platform?: PuppetPlatform): Record<AgentType, AgentPreset> {
  const resolved = platform ?? detectPlatform();
  return Object.fromEntries(
    AgentTypeSchema.options.map((type) => [type, buildPreset(type, resolved)]),
  ) as Record<AgentType, AgentPreset>;
}
