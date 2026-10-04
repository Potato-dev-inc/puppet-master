/** Shared types for the legacy Node MCP server. */

export interface BridgeClient {
  baseUrl: string;
  sessionId: string;
}

/** Mutable holder so a refreshed client (new bridge port) is seen by every caller. */
export interface ClientRef {
  current: BridgeClient;
}

export interface RegistryTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  visibility?: { external_mcp?: boolean };
  method?: string;
  path?: string;
  outputSchema?: Record<string, unknown>;
}

export interface BridgeToolError { code: string; message: string; recoverable: boolean; retry_after_ms?: number; context?: unknown }
