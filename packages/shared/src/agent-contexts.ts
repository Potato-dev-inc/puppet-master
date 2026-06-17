import type { AgentType } from './agents.js';

export type AgentCapability =
  | 'codebase-reasoning'
  | 'implementation'
  | 'review'
  | 'debugging'
  | 'research'
  | 'terminal-ops'
  | 'ui-orchestration';

export interface AgentContextProfile {
  agent_type: AgentType;
  label: string;
  default_model: string | null;
  model_detection: 'cli-banner' | 'configuration' | 'unknown';
  smartness: number;
  strengths: AgentCapability[];
  context_notes: string[];
  best_for: string[];
  planned_sidebar_actions: string[];
}

export interface AgentModelInspection {
  pane_id: string;
  agent_type: AgentType;
  detected_model: string | null;
  source: 'buffer' | 'profile' | 'unknown';
  confidence: 'high' | 'medium' | 'low';
  smartness: number;
  notes: string[];
}

const MODEL_PATTERNS: Array<{ pattern: RegExp; normalize?: (match: RegExpMatchArray) => string }> = [
  { pattern: /\b(gpt-5(?:\.[\w-]+)?|gpt-4(?:\.[\w-]+)?|o[134](?:-[\w-]+)?)\b/i },
  { pattern: /\b(claude-(?:opus|sonnet|haiku)-[\w.-]+)\b/i },
  { pattern: /\b((?:gemini|qwen|deepseek|llama|mistral|kimi)[\w./:-]*)\b/i },
  {
    pattern: /\bmodel(?:\s+is|\s*[:=])\s*([A-Za-z0-9_./:-]+)/i,
    normalize: (match) => match[1],
  },
];

export const AGENT_CONTEXT_PROFILES: Record<AgentType, AgentContextProfile> = {
  claude: {
    agent_type: 'claude',
    label: 'Claude Code',
    default_model: null,
    model_detection: 'cli-banner',
    smartness: 9,
    strengths: ['codebase-reasoning', 'implementation', 'review', 'debugging'],
    context_notes: [
      'Strong default for broad repository understanding, multi-file edits, and code review.',
      'Usually exposes its active model in the TUI banner or startup text when configured by the CLI.',
    ],
    best_for: ['planning complex changes', 'reviewing diffs', 'large refactors', 'implementation with tests'],
    planned_sidebar_actions: ['delegate task', 'ask for review', 'compare plan against Codex', 'summarize current pane'],
  },
  codex: {
    agent_type: 'codex',
    label: 'Codex CLI',
    default_model: null,
    model_detection: 'cli-banner',
    smartness: 9,
    strengths: ['implementation', 'debugging', 'terminal-ops', 'codebase-reasoning'],
    context_notes: [
      'Good default for surgical coding, build fixes, and terminal-native verification loops.',
      'Model may be supplied by the Codex config or surfaced in the TUI; inspect terminal buffer for the best available signal.',
    ],
    best_for: ['coding fixes', 'running tests', 'debugging build failures', 'iterative verification'],
    planned_sidebar_actions: ['delegate implementation', 'run verification loop', 'ask for status', 'handoff focused bug'],
  },
  opencode: {
    agent_type: 'opencode',
    label: 'OpenCode',
    default_model: null,
    model_detection: 'configuration',
    smartness: 7,
    strengths: ['implementation', 'terminal-ops', 'debugging'],
    context_notes: [
      'Useful as an additional coding terminal when you want parallel exploration.',
      'Model routing is provider-config dependent, so treat detection as advisory unless the buffer shows an explicit model.',
    ],
    best_for: ['parallel edits', 'alternative implementation passes', 'lighter bug fixes'],
    planned_sidebar_actions: ['delegate parallel attempt', 'ask for alternative', 'compare output'],
  },
  powershell: {
    agent_type: 'powershell',
    label: 'PowerShell',
    default_model: null,
    model_detection: 'unknown',
    smartness: 1,
    strengths: ['terminal-ops'],
    context_notes: ['Plain shell pane. It has no model; use it for deterministic commands and scripts.'],
    best_for: ['build commands', 'file inspection', 'manual scripts'],
    planned_sidebar_actions: ['run command', 'capture output', 'prepare environment'],
  },
  bash: {
    agent_type: 'bash',
    label: 'Bash',
    default_model: null,
    model_detection: 'unknown',
    smartness: 1,
    strengths: ['terminal-ops'],
    context_notes: ['Plain shell pane. It has no model; use it for POSIX-flavored commands.'],
    best_for: ['shell commands', 'cross-platform scripts', 'log inspection'],
    planned_sidebar_actions: ['run command', 'capture output', 'prepare environment'],
  },
  cursor: {
    agent_type: 'cursor',
    label: 'Cursor IDE',
    default_model: null,
    model_detection: 'unknown',
    smartness: 6,
    strengths: ['ui-orchestration', 'implementation'],
    context_notes: [
      'IDE launcher rather than a terminal TUI. Treat model information as unavailable unless a future Cursor bridge reports it.',
    ],
    best_for: ['opening the workspace visually', 'manual user-guided edits'],
    planned_sidebar_actions: ['open project', 'focus editor', 'handoff manual review'],
  },
};

export function listAgentContextProfiles(): AgentContextProfile[] {
  return Object.values(AGENT_CONTEXT_PROFILES);
}

export function getAgentContextProfile(agentType: AgentType): AgentContextProfile {
  return AGENT_CONTEXT_PROFILES[agentType];
}

export function detectModelFromBuffer(buffer: string): string | null {
  for (const { pattern, normalize } of MODEL_PATTERNS) {
    const match = buffer.match(pattern);
    if (match) return normalize ? normalize(match) : match[1];
  }
  return null;
}

export function inspectAgentModel(
  paneId: string,
  agentType: AgentType,
  buffer: string,
): AgentModelInspection {
  const profile = getAgentContextProfile(agentType);
  const detected = detectModelFromBuffer(buffer);
  if (detected) {
    return {
      pane_id: paneId,
      agent_type: agentType,
      detected_model: detected,
      source: 'buffer',
      confidence: 'medium',
      smartness: profile.smartness,
      notes: ['Detected from recent terminal buffer text; confirm if the CLI allows runtime model switching.'],
    };
  }

  return {
    pane_id: paneId,
    agent_type: agentType,
    detected_model: profile.default_model,
    source: profile.default_model ? 'profile' : 'unknown',
    confidence: profile.default_model ? 'low' : 'low',
    smartness: profile.smartness,
    notes: profile.default_model
      ? ['Using the static agent profile default because no model was visible in the buffer.']
      : ['No model was visible in the buffer and this agent has no static default.'],
  };
}
