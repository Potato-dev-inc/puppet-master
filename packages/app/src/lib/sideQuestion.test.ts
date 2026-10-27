import { describe, expect, it } from 'vitest';
import { groupTurns, type ChatItem } from './sessionChatModel';
import { buildSideContext, SIDE_CONTEXT_LIMIT } from './sideQuestion';

let n = 0;
const id = () => `${++n}:0`;

/** One finished turn: the person's message, the agent's text, one tool call with its result. */
function turn(user: string, reply: string, tool = 'Bash ls', ok = true): ChatItem[] {
  const call = `c${id()}`;
  return [
    { kind: 'user', id: id(), at: null, text: user },
    { kind: 'assistant', id: id(), at: null, text: reply, model: null, finalAnswer: false },
    { kind: 'toolCall', id: id(), at: null, callId: call, name: 'Bash', summary: tool },
    { kind: 'toolResult', id: id(), at: null, callId: call, ok, output: 'SECRET TOOL OUTPUT' },
    { kind: 'turnEnd', id: id(), at: null, outcome: 'completed', durationMs: 1000 },
  ];
}

const turnsOf = (...parts: ChatItem[][]) => groupTurns(parts.flat()).turns;

describe('buildSideContext', () => {
  it('lists the turns oldest first with the person, the agent and a line per tool call', () => {
    const text = buildSideContext(turnsOf(turn('first ask', 'first reply'), turn('second ask', 'second reply', 'Write sideq.txt')));
    expect(text).toBe(
      ['Person: first ask', 'Agent: first reply', 'Tool: Bash ls', '', 'Person: second ask', 'Agent: second reply', 'Tool: Write sideq.txt'].join('\n'),
    );
  });

  it('leaves tool output out and marks a failed call', () => {
    const text = buildSideContext(turnsOf(turn('go', 'trying', 'npm test', false)));
    expect(text).not.toContain('SECRET TOOL OUTPUT');
    expect(text).toContain('Tool: npm test (failed)');
  });

  it('keeps the newest turns within the cap and says how many it left out', () => {
    const turns = turnsOf(...Array.from({ length: 40 }, (_, k) => turn(`ask ${k}`, `reply ${k} ${'x'.repeat(900)}`)));
    const text = buildSideContext(turns);
    expect(Array.from(text).length).toBeLessThanOrEqual(SIDE_CONTEXT_LIMIT + 60);
    expect(text).toMatch(/^\[\d+ earlier turns left out\]/);
    expect(text).toContain('Person: ask 39');
    expect(text).not.toContain('Person: ask 0\n');
    // The kept turns are whole and in order.
    expect(text.indexOf('ask 38')).toBeLessThan(text.indexOf('ask 39'));
  });

  it('cuts one huge message so the newest turn still fits', () => {
    const text = buildSideContext(turnsOf(turn('small', 'tiny'), turn('big', 'y'.repeat(50_000))));
    expect(Array.from(text).length).toBeLessThan(SIDE_CONTEXT_LIMIT);
    expect(text).toContain('Person: big');
    expect(text).toContain('Person: small');
  });

  it('uses a smaller cap when given one and keeps the end of a single oversized block', () => {
    const text = buildSideContext(turnsOf(turn('a', 'b'.repeat(500))), 100);
    expect(Array.from(text).length).toBeLessThanOrEqual(100);
    expect(text.endsWith('Tool: Bash ls')).toBe(true);
  });

  it('is empty for an empty chat', () => {
    expect(buildSideContext([])).toBe('');
  });
});
