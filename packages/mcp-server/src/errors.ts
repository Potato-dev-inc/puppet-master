/** Structured tool errors: bridge HTTP failures and argument-validation classification. */
import type { BridgeToolError } from './types.js';

export class ToolCallError extends Error {
  constructor(readonly detail: BridgeToolError) { super(detail.message); }
}

export function bridgeHttpToolError(status: number, body: string, route: string): ToolCallError {
  try {
    const parsed = JSON.parse(body) as BridgeToolError & { error?: unknown };
    if (typeof parsed.code === 'string' && typeof parsed.recoverable === 'boolean') {
      const message = typeof parsed.message === 'string' && parsed.message
        ? parsed.message
        : typeof parsed.error === 'string' && parsed.error
          ? parsed.error
          : `bridge request failed: ${route}`;
      return new ToolCallError({ ...parsed, message });
    }
  } catch { /* map legacy `{error}` bodies below */ }
  const mapping: Record<number, [string, boolean, number?]> = {
    400: ['INVALID_ARGUMENT', false], 401: ['AUTHORIZATION_DENIED', false], 403: ['AUTHORIZATION_DENIED', false],
    404: ['NOT_FOUND', false], 409: ['CONFLICT', true, 1000], 429: ['RATE_LIMITED', true, 1000],
  };
  const [code, recoverable, retry_after_ms] = mapping[status] ?? (status >= 500 ? ['BRIDGE_UNAVAILABLE', true] : ['BRIDGE_ERROR', true]);
  let message = body;
  try { const parsed = JSON.parse(body) as { error?: unknown }; if (typeof parsed.error === 'string') message = parsed.error; } catch { /* retain plain response */ }
  return new ToolCallError({ code, message: message || `bridge request failed: ${route}`, recoverable, ...(retry_after_ms === undefined ? {} : { retry_after_ms }), context: { http_status: status, route } });
}

export function classifyToolError(error: unknown, message: string, tool: string): BridgeToolError {
  const invalid = error instanceof Error && (error.name === 'ZodError' || /missing required|unknown tool|unknown agent_type|refusing to target|must be/i.test(message));
  return { code: invalid ? 'INVALID_ARGUMENT' : 'bridge_error', message, recoverable: !invalid, context: { tool } };
}
