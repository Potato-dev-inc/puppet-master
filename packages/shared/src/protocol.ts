import { z } from 'zod';
import { AgentTypeSchema } from './agents.js';

export const PaneStatusSchema = z.enum(['running', 'waiting_input', 'error', 'idle']);
export type PaneStatus = z.infer<typeof PaneStatusSchema>;

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
});
export type WriteInputRequest = z.infer<typeof WriteInputRequestSchema>;

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
export const DEFAULT_BRIDGE_HOST = '127.0.0.1';
export const BRIDGE_HTTP_PORT_RANGE = { min: 17321, max: 17399 };