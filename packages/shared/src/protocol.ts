import { z } from 'zod';
import { AgentTypeSchema } from './agents.js';

export const PaneStatusSchema = z.enum(['running', 'waiting_input', 'error', 'idle']);
export type PaneStatus = z.infer<typeof PaneStatusSchema>;

/** Linux Inside: where a pane runs when it is inside WSL. Absent for a Windows pane. Display and chat facts only. */
export const PanePlaceSchema = z.object({
  kind: z.string(),
  distro: z.string(),
  cwd: z.string(),
  mount_root: z.string(),
  unc_root: z.string(),
  home_windows: z.string().optional(),
  warnings: z.array(z.string()).optional(),
});
export type PanePlace = z.infer<typeof PanePlaceSchema>;

export const PaneInfoSchema = z.object({
  id: z.string(),
  agent_type: AgentTypeSchema,
  pid: z.number().int(),
  status: PaneStatusSchema,
  created_at: z.number(),
  last_output_at: z.number().nullable(),
  cwd: z.string(),
  cols: z.number().int(),
  rows: z.number().int(),
  /** A phone viewing this pane holds its size; desktop windows show a notice meanwhile. */
  viewer_held: z.boolean().optional(),
  /** The agent reprints its whole conversation on every resize; viewers wipe scrollback at each one. */
  reprints_on_resize: z.boolean().optional(),
  /** Human title (user name, else derived from the first task); absent until the pane has one. */
  title: z.string().nullable().optional(),
  /** The pane of the agent that started this one over MCP (a helper); absent for a pane nobody's agent started. Display-only. */
  parent_pane_id: z.string().nullable().optional(),
  /** Linux Inside: the pane runs inside WSL. Absent for a Windows pane. */
  place: PanePlaceSchema.optional(),
});
export type PaneInfo = z.infer<typeof PaneInfoSchema>;

export const SpawnPaneRequestSchema = z.object({
  agent_type: AgentTypeSchema,
  cwd: z.string().optional(),
  cols: z.number().int().positive().default(120),
  rows: z.number().int().positive().default(30),
  extra_args: z.array(z.string()).optional(),
  pane_id: z.string().optional(),
});
export type SpawnPaneRequest = z.infer<typeof SpawnPaneRequestSchema>;

export const WriteInputRequestSchema = z.object({
  text: z.string(),
  append_newline: z.boolean().default(true),
  via_opencode_api: z.boolean().optional(),
  model_provider: z.string().optional(),
  model_id: z.string().optional(),
});
export type WriteInputRequest = z.infer<typeof WriteInputRequestSchema>;

export const SwitchModelRequestSchema = z.object({
  model_id: z.string().min(1),
  model_provider: z.string().optional(),
});
export type SwitchModelRequest = z.infer<typeof SwitchModelRequestSchema>;

export const McpLogEntrySchema = z.object({
  id: z.string(),
  timestamp: z.number(),
  source: z.enum(['builtin', 'external']),
  tool: z.string(),
  args: z.unknown(),
  result_preview: z.string().optional(),
  error: z.string().optional(),
  duration_ms: z.number().optional(),
});
export type McpLogEntry = z.infer<typeof McpLogEntrySchema>;

export const BRIDGE_PORT_FILE_ENV = 'PUPPET_MASTER_BRIDGE_PORT_FILE';
export const DEFAULT_BRIDGE_PORT_FILE = 'puppet-master.bridge.port';
/** Per-launch local secret the app writes beside the port file; sent as `X-PM-Local-Secret`. */
export const BRIDGE_LOCAL_SECRET_FILE = 'puppet-master.bridge.secret';
export const BRIDGE_LOCAL_SECRET_HEADER = 'X-PM-Local-Secret';
export const DEFAULT_BRIDGE_HOST = '127.0.0.1';
export const BRIDGE_HTTP_PORT_RANGE = { min: 17321, max: 17399 };

// --- PWA Orchestrator messaging ---

export const OrchestratorUserMessageSchema = z.object({
  text: z.string(),
  message_id: z.string(),
});
export type OrchestratorUserMessage = z.infer<typeof OrchestratorUserMessageSchema>;

export const OrchestratorChatEventSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('user'),  message_id: z.string(), text: z.string() }),
  z.object({ type: z.literal('text'),  message_id: z.string(), text: z.string() }),
  z.object({ type: z.literal('tool'),  message_id: z.string(), tool: z.string(), result: z.string().optional(), error: z.string().optional() }),
  z.object({ type: z.literal('done'),  message_id: z.string() }),
  z.object({ type: z.literal('error'), message_id: z.string(), error: z.string() }),
]);
export type OrchestratorChatEvent = z.infer<typeof OrchestratorChatEventSchema>;