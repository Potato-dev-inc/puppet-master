import { z } from 'zod';

export const ChatMessageSchema = z.object({
  id: z.string(),
  role: z.enum(['user', 'assistant', 'system', 'tool']),
  content: z.string(),
  tool_calls: z.array(z.object({
    id: z.string(),
    name: z.string(),
    args: z.unknown(),
  })).optional(),
  tool_call_id: z.string().optional(),
  timestamp: z.number(),
});
export type ChatMessage = z.infer<typeof ChatMessageSchema>;

export const LlmProviderSchema = z.enum(['anthropic', 'openai', 'openrouter']);
export type LlmProvider = z.infer<typeof LlmProviderSchema>;

export const LlmModelSchema = z.object({
  provider: LlmProviderSchema,
  model_id: z.string().min(1),
  label: z.string().min(1),
});
export type LlmModel = z.infer<typeof LlmModelSchema>;

/** How the Puppet Master sidebar drives orchestration. */
export const OrchestratorBackendSchema = z.enum([
  'api',           // Direct LLM API + in-process MCP tool loop (current)
  'claude_cli',    // Dedicated Claude Code pane with MCP configured
  'codex_cli',     // Dedicated Codex CLI pane with MCP configured
  'opencode_cli',  // Dedicated OpenCode pane with MCP configured
]);
export type OrchestratorBackend = z.infer<typeof OrchestratorBackendSchema>;

export const ORCHESTRATOR_BACKEND_LABELS: Record<OrchestratorBackend, string> = {
  api: 'LLM API (direct)',
  claude_cli: 'Claude Code CLI',
  codex_cli: 'Codex CLI',
  opencode_cli: 'OpenCode CLI',
};

export function modelKey(model: Pick<LlmModel, 'provider' | 'model_id'>): string {
  return `${model.provider}::${model.model_id}`;
}

export function parseModelKey(key: string): { provider: LlmProvider; model_id: string } | null {
  const idx = key.indexOf('::');
  if (idx <= 0) return null;
  const provider = key.slice(0, idx);
  const model_id = key.slice(idx + 2);
  const parsed = LlmProviderSchema.safeParse(provider);
  if (!parsed.success || !model_id) return null;
  return { provider: parsed.data, model_id };
}

export const DEFAULT_LLM_MODELS: LlmModel[] = [
  { provider: 'anthropic', model_id: 'claude-sonnet-4-6', label: 'Claude Sonnet 4.6' },
  { provider: 'anthropic', model_id: 'claude-opus-4-6', label: 'Claude Opus 4.6' },
  { provider: 'openai', model_id: 'gpt-4.1', label: 'GPT-4.1' },
  { provider: 'openai', model_id: 'gpt-4.1-mini', label: 'GPT-4.1 mini' },
  { provider: 'openrouter', model_id: 'anthropic/claude-sonnet-4', label: 'Claude Sonnet 4 (OR)' },
  { provider: 'openrouter', model_id: 'openai/gpt-4.1', label: 'GPT-4.1 (OR)' },
  { provider: 'openrouter', model_id: 'google/gemini-2.5-flash-preview', label: 'Gemini 2.5 Flash (OR)' },
  { provider: 'openrouter', model_id: 'meta-llama/llama-3.3-70b-instruct', label: 'Llama 3.3 70B (OR)' },
];

export const SettingsSchema = z.object({
  anthropic_api_key: z.string().optional(),
  openai_api_key: z.string().optional(),
  openrouter_api_key: z.string().optional(),
  default_provider: LlmProviderSchema.default('anthropic'),
  default_model: z.string().default('claude-sonnet-4-6'),
  /** User-defined models merged with presets in the sidebar picker. */
  custom_models: z.array(LlmModelSchema).default([]),
  /** Sidebar orchestration backend. API runs an in-process tool loop; CLI modes delegate to live agent panes. */
  orchestrator_backend: OrchestratorBackendSchema.default('api'),
  /** Stable pane id for the dedicated orchestrator CLI pane (CLI backends). */
  orchestrator_pane_id: z.string().optional(),
  /** Mobile terminal text buffer delay before committing to the PTY (autocorrect settling). */
  mobile_input_delay_ms: z.number().int().min(0).max(1000).default(250),
  /** Show the mobile terminal command input bar. When false, the tap target remains invisible. */
  mobile_input_visible: z.boolean().default(true),
  project_path: z.string().optional(),
  /** Public HTTPS origin for mobile PWA (reverse proxy target). Used in pairing QR. */
  public_pwa_url: z.string().optional(),
  /** Local Vite dev server port the public URL should proxy to (default 1420). */
  dev_server_port: z.number().int().min(1024).max(65535).default(1420),
  /** Orchestrator sidebar width in the workspace (px). */
  sidebar_width: z.number().int().min(300).max(800).default(360),
  /** Desktop UI chrome theme. */
  theme: z.enum(['dark', 'light']).default('dark'),
  /** Developer option: install MCP configs to launch the bundled Rust binary instead of npm. */
  developer_use_rust_mcp: z.boolean().default(false),
});
export type Settings = z.infer<typeof SettingsSchema>;
