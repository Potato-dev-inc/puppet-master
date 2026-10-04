/** Small helpers shared across the MCP server modules. */

// CRITICAL: every log line goes to stderr, never stdout, or the JSON-RPC stream is corrupted.
export const log = (...args: unknown[]) => {
  process.stderr.write(`[puppet-master-mcp] ${args.map(String).join(' ')}\n`);
};

export function tryJson(text: string): unknown | undefined {
  try { return JSON.parse(text); } catch { return undefined; }
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
