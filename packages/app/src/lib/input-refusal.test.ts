import { describe, expect, it } from 'vitest';
import {
  AGENT_RUN_BY_ANOTHER_AGENT,
  INPUT_NOT_SENT,
  inputRefusalNote,
  isInputRefusal,
  PHONE_NOT_ALLOWED_TO_TYPE,
} from './input-refusal';

// The two real refusals, as the bridge sent them to the loopback PWA tab (VERIFY-R11, evidence
// phone-loopback-run.txt / gate-curl.txt): the HTTP body and the socket frame's `error` text.
const HTTP_403 =
  'bridge POST /panes/8cf2ad53/input -> 403: {"code":"AUTHORIZATION_DENIED","error":"managed pane mutations require an MCP session id","recoverable":false}';
const WS_ERROR = 'managed pane mutations require an MCP session id';

describe('inputRefusalNote', () => {
  it('says why in plain words for the managed-pane guard, over HTTP and over the socket', () => {
    expect(inputRefusalNote(new Error(HTTP_403))).toBe(AGENT_RUN_BY_ANOTHER_AGENT);
    expect(inputRefusalNote(WS_ERROR)).toBe(AGENT_RUN_BY_ANOTHER_AGENT);
    expect(AGENT_RUN_BY_ANOTHER_AGENT).toBe(
      'This agent is being run by another agent. Answer from the card or open it on the PC.',
    );
  });

  it('tells a phone without the typing ability, and never stays silent for anything else', () => {
    expect(inputRefusalNote('not_allowed')).toBe(PHONE_NOT_ALLOWED_TO_TYPE);
    expect(inputRefusalNote('pane not found')).toBe(INPUT_NOT_SENT);
    expect(inputRefusalNote(undefined)).toBe(INPUT_NOT_SENT);
  });
});

describe('isInputRefusal', () => {
  it('is true for a 403 on the input or key route only', () => {
    expect(isInputRefusal(new Error(HTTP_403))).toBe(true);
    expect(isInputRefusal(new Error('bridge POST /panes/p1/key -> 403: {}'))).toBe(true);
    expect(isInputRefusal(new Error('bridge POST /panes/p1/input -> 500: boom'))).toBe(false);
    expect(isInputRefusal(new Error('Failed to fetch'))).toBe(false);
    expect(isInputRefusal('nope')).toBe(false);
  });
});
