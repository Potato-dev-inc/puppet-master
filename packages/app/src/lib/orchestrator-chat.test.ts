import { describe, expect, it } from 'vitest';
import { applyOrchestratorChatEvent } from './orchestrator-chat';

describe('applyOrchestratorChatEvent', () => {
  it('dedupes user events', () => {
    const first = applyOrchestratorChatEvent(
      { type: 'user', message_id: 'a', text: 'hi' },
      [],
    );
    const second = applyOrchestratorChatEvent(
      { type: 'user', message_id: 'a', text: 'hi' },
      first,
    );
    expect(second).toHaveLength(1);
  });

  it('streams assistant text on the same message id', () => {
    let lines = applyOrchestratorChatEvent({ type: 'text', message_id: 'a', text: 'hel' }, []);
    lines = applyOrchestratorChatEvent({ type: 'text', message_id: 'a', text: 'lo' }, lines);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.text).toBe('hello');
    expect(lines[0]?.streaming).toBe(true);
  });
});
