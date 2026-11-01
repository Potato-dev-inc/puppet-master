// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { ACP_BADGE, ACP_PERMISSION_KIND, permissionCards, runStrip, stoppedStrip, type AcpChatState } from './acpChat';
import type { Question } from './questions';

const q = (id: string, over: Partial<Question> = {}): Question => ({
  id,
  question: 'Codex asks permission: Write notes.txt',
  choices: [{ id: 'allow_once', label: 'Allow Once' }],
  allow_free_text: false,
  urgency: 'high',
  created_at_ms: 1,
  expires_at_ms: 2,
  state: 'pending',
  kind: ACP_PERMISSION_KIND,
  pane_id: 'p1',
  ...over,
});

const state = (over: Partial<AcpChatState> = {}): AcpChatState => ({
  lane: 'acp',
  agent: 'codex',
  label: 'Codex',
  phase: 'working',
  status: 'running',
  turnInFlight: true,
  stopping: false,
  canSteer: false,
  turn: 1,
  title: null,
  usage: null,
  permissions: [],
  closed: false,
  ...over,
});

describe('permissionCards', () => {
  it('keeps the pending permission questions of this pane and nothing else', () => {
    const list = [q('a'), q('b', { pane_id: 'p2' }), q('c', { kind: undefined }), q('d', { state: 'answered' }), q('e', { kind: 'budget' })];
    expect(permissionCards(list, 'p1').map((x) => x.id)).toEqual(['a']);
  });
});

describe('runStrip', () => {
  it('has nothing to say while nothing runs or the chat is closed', () => {
    expect(runStrip(null)).toBeNull();
    expect(runStrip(state({ turnInFlight: false, phase: 'idle' }))).toBeNull();
    expect(runStrip(state({ closed: true }))).toBeNull();
  });

  it('says what the session does, and reads Stopping… from the first press until the turn ends', () => {
    expect(runStrip(state())).toEqual({ text: 'Working…', stopping: false });
    expect(runStrip(state({ phase: 'needs_input' }))).toEqual({ text: 'Waiting for your answer', stopping: false });
    expect(runStrip(state(), true)).toEqual({ text: 'Stopping…', stopping: true });
    expect(runStrip(state({ phase: 'cancelling', stopping: true }))).toEqual({ text: 'Stopping…', stopping: true });
    // The turn ended: the press is over and there is no strip.
    expect(runStrip(state({ turnInFlight: false, phase: 'idle' }), true)).toBeNull();
  });
});

describe('the badge', () => {
  it('names the way to run, not an agent', () => {
    expect(ACP_BADGE).toBe('Chat (ACP)');
    expect(ACP_BADGE.toLowerCase()).not.toContain('claude');
  });
});

describe('stoppedStrip', () => {
  const base = { lane: 'acp', agent: 'codex', label: 'Codex', phase: 'exited', status: 'error', turnInFlight: false, stopping: false, canSteer: false, turn: 1, title: null, usage: null, permissions: [], closed: false } as AcpChatState;
  it('says why the chat stopped, with the exit code when there is one', () => {
    expect(stoppedStrip({ ...base, exit: { kind: 'crashed', code: 3, reason: null } })?.text).toBe('The chat stopped: the agent stopped unexpectedly (exit code 3)');
    expect(stoppedStrip({ ...base, exit: { kind: 'hung', code: null, reason: null } })?.text).toBe('The chat stopped: the agent stopped answering');
    expect(stoppedStrip({ ...base, exit: { kind: 'start_failed', code: null, reason: 'no such   file\nat x' } })?.text).toBe('The chat stopped: the agent could not start: no such file at x');
  });
  it('is absent while it runs, after a close on purpose, and for a closed pane', () => {
    expect(stoppedStrip(null)).toBeNull();
    expect(stoppedStrip({ ...base, phase: 'idle', exit: null })).toBeNull();
    expect(stoppedStrip({ ...base, exit: { kind: 'closed', code: null, reason: null } })).toBeNull();
    expect(stoppedStrip({ ...base, closed: true, exit: { kind: 'crashed', code: 1, reason: null } })).toBeNull();
  });
});
