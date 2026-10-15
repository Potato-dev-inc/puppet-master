import { z } from 'zod';
import { detectPlatform, type PuppetPlatform } from './platform.js';

/** The code agents compiled into the app (SPEC UI-1). Other ids come from the runtime registry. */
export const BUILTIN_AGENT_TYPES = [
  'claude',
  'codex',
  'opencode',
  'opencode_native',
  'cmd',
  'powershell',
  'bash',
  'cursor',
  'cursor_agent',
] as const;
export type BuiltinAgentType = (typeof BUILTIN_AGENT_TYPES)[number];

/** Agent id pattern shared with the backend registry (SPEC VAL-1). */
export const AGENT_ID_PATTERN = /^[a-z][a-z0-9_]{1,31}$/;

/** Any pattern-valid agent id; whether it is registered is the backend's call. */
export const AgentTypeSchema = z
  .string()
  .regex(AGENT_ID_PATTERN, 'agent_type must match ^[a-z][a-z0-9_]{1,31}$');
export type AgentType = string;

export function isBuiltinAgentType(id: string): id is BuiltinAgentType {
  return (BUILTIN_AGENT_TYPES as readonly string[]).includes(id);
}

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
  BuiltinAgentType,
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
  opencode_native: {
    type: 'opencode_native',
    label: 'OpenCode (API)',
    isTui: true,
    icon: 'OC+',
  },
  cmd: {
    type: 'cmd',
    label: 'Command Prompt',
    isTui: true,
    icon: 'CMD',
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
  cursor_agent: {
    type: 'cursor_agent',
    label: 'Cursor Agent CLI',
    isTui: true,
    icon: 'CA',
  },
};

const CODEX_ARGS = ['--sandbox', 'workspace-write', '--ask-for-approval', 'never'];
/** Spawn UI: OpenCode API workers plus PowerShell (multiple panes of either type). */
const AGENT_LAUNCH_TYPES: BuiltinAgentType[] = ['opencode_native', 'powershell'];

const PLATFORM_COMMANDS: Record<PuppetPlatform, Record<BuiltinAgentType, AgentCommandSpec>> = {
  windows: {
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
    opencode_native: {
      command: 'opencode',
      baseArgs: [],
      description: 'OpenCode API worker (serve + attach TUI)',
    },
    cmd: {
      command: 'cmd.exe',
      baseArgs: ['/K'],
      description: 'Windows Command Prompt',
    },
    powershell: {
      command: 'powershell.exe',
      baseArgs: ['-NoLogo', '-NoExit'],
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
    cursor_agent: { command: 'cursor-agent', baseArgs: [], description: 'Cursor Agent CLI' },
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
    opencode_native: {
      command: 'opencode',
      baseArgs: [],
      description: 'OpenCode API worker (serve + attach TUI)',
    },
    cmd: {
      command: 'zsh',
      baseArgs: ['-l'],
      description: 'Default login shell',
    },
    powershell: {
      command: 'pwsh',
      baseArgs: ['-NoLogo', '-NoExit'],
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
    cursor_agent: { command: 'cursor-agent', baseArgs: [], description: 'Cursor Agent CLI' },
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
    opencode_native: {
      command: 'opencode',
      baseArgs: [],
      description: 'OpenCode API worker (serve + attach TUI)',
    },
    cmd: {
      command: 'bash',
      baseArgs: ['--login'],
      description: 'Default login shell',
    },
    powershell: {
      command: 'pwsh',
      baseArgs: ['-NoLogo', '-NoExit'],
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
    cursor_agent: { command: 'cursor-agent', baseArgs: [], description: 'Cursor Agent CLI' },
  },
};

function buildPreset(type: BuiltinAgentType, platform: PuppetPlatform): AgentPreset {
  const meta = AGENT_META[type];
  const spec = PLATFORM_COMMANDS[platform][type];
  return {
    ...meta,
    description: spec.description,
    command: spec.command,
    baseArgs: spec.baseArgs,
  };
}

/** Neutral preset for an id outside the compiled set (SPEC UI-1). */
function neutralPreset(id: AgentType): AgentPreset {
  return {
    type: id,
    label: id,
    description: '',
    command: id,
    baseArgs: [],
    isTui: true,
    icon: id.slice(0, 2).toUpperCase(),
  };
}

/** Preset for any agent id: the built-in one, else a neutral fallback labelled with the id. */
export function presetFor(id: AgentType, platform?: PuppetPlatform): AgentPreset {
  return isBuiltinAgentType(id) ? buildPreset(id, platform ?? detectPlatform()) : neutralPreset(id);
}

export function getPreset(type: AgentType, platform?: PuppetPlatform): AgentPreset {
  return presetFor(type, platform);
}

export function listPresets(platform?: PuppetPlatform): AgentPreset[] {
  const resolved = platform ?? detectPlatform();
  return BUILTIN_AGENT_TYPES.map((type) => buildPreset(type, resolved));
}

export function getDefaultTerminalAgentType(_platform?: PuppetPlatform): AgentType {
  return 'opencode_native';
}

export function listLaunchPresets(platform?: PuppetPlatform): AgentPreset[] {
  const resolved = platform ?? detectPlatform();
  return AGENT_LAUNCH_TYPES.map((type) => buildPreset(type, resolved));
}

export function getAgentPresets(platform?: PuppetPlatform): Record<BuiltinAgentType, AgentPreset> {
  const resolved = platform ?? detectPlatform();
  return Object.fromEntries(
    BUILTIN_AGENT_TYPES.map((type) => [type, buildPreset(type, resolved)]),
  ) as Record<BuiltinAgentType, AgentPreset>;
}
