// When the PC refuses typed input (an HTTP 403 from the managed-pane guard, or a WebSocket
// `input-error` frame), the person must be told: a refused write is never silent.
//
// The bridge refuses a caller that is not a paired phone when the pane belongs to an agent run
// (`mcp_sessions/authorize.rs`: "managed pane mutations require an MCP session id"). That is a
// loopback PWA tab on the PC itself (dev, demos, filming); a paired phone is exempt.

/** Plain words, no ids: what happened and what to do instead. */
export const AGENT_RUN_BY_ANOTHER_AGENT =
  'This agent is being run by another agent. Answer from the card or open it on the PC.';
export const PHONE_NOT_ALLOWED_TO_TYPE = "This phone can't type into agents. Allow typing for it on the PC.";
export const INPUT_NOT_SENT = "That wasn't sent to the agent.";

/** What the refusal is about, from the PC's error text (an Error message or a socket frame's `error`). */
export function inputRefusalNote(detail: unknown): string {
  const text = typeof detail === 'string' ? detail : detail instanceof Error ? detail.message : JSON.stringify(detail ?? '');
  if (/managed (agent|pane)|AUTHORIZATION_DENIED|require an MCP session/i.test(text)) return AGENT_RUN_BY_ANOTHER_AGENT;
  if (/not_allowed|DEVICE_NOT_ALLOWED/i.test(text)) return PHONE_NOT_ALLOWED_TO_TYPE;
  return INPUT_NOT_SENT;
}

/** Whether a failed `POST /panes/{id}/input` was the PC refusing (403), not a dropped link. */
export function isInputRefusal(error: unknown): boolean {
  return error instanceof Error && /\/input -> 403:|\/key -> 403:/.test(error.message);
}
