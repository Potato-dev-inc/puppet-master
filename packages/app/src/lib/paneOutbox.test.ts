import { describe, expect, it } from 'vitest';
import { bubblesFor, composerBlock, errorText, SENT_BUBBLE_MS, withSent, type OutboxMessage } from './paneOutbox';

function msg(id: string, state: OutboxMessage['state'], text = id, createdAt = 1): OutboxMessage {
  return { id, text, createdAt, state, waitingFor: null, error: state === 'failed' ? 'It failed.' : null };
}

const none = new Map<string, number>();
const nobody = new Set<string>();

describe('bubblesFor', () => {
  it('shows waiting, sending and failed messages, and not cancelled ones', () => {
    const list = [msg('a', 'queued'), msg('b', 'sending'), msg('c', 'failed'), msg('d', 'cancelled')];
    expect(bubblesFor(list, [], none, nobody, 0).map((m) => m.id)).toEqual(['a', 'b', 'c']);
  });

  it('drops a sent message as soon as the conversation shows the same words', () => {
    const list = [msg('a', 'sent', 'Please  run the\ntests')];
    expect(bubblesFor(list, [], none, nobody, 0)).toHaveLength(1);
    expect(bubblesFor(list, ['Please run the tests'], none, nobody, 0)).toHaveLength(0);
  });

  it('keeps a sent message for ten seconds from when it was first seen sent', () => {
    const list = [msg('a', 'sent')];
    const seen = new Map([['a', 1000]]);
    expect(bubblesFor(list, [], seen, nobody, 1000 + SENT_BUBBLE_MS - 1)).toHaveLength(1);
    expect(bubblesFor(list, [], seen, nobody, 1000 + SENT_BUBBLE_MS)).toHaveLength(0);
  });

  it('leaves out the messages the person hid', () => {
    const list = [msg('a', 'failed'), msg('b', 'failed')];
    expect(bubblesFor(list, [], none, new Set(['a']), 0).map((m) => m.id)).toEqual(['b']);
  });
});

describe('composerBlock', () => {
  it('says why the box is off, in plain words', () => {
    expect(composerBlock({ status: 'idle' }, false)).toBeNull();
    expect(composerBlock({ status: 'running' }, false)).toBeNull();
    expect(composerBlock({ status: 'waiting_input' }, false)).toBeNull();
    expect(composerBlock({ status: 'idle' }, true)).toBe('Chat replies need an agent pane.');
    expect(composerBlock({ status: 'error' }, false)).toBe('The agent is not running. Open Terminal.');
    expect(composerBlock(undefined, false)).toBe('This pane is not open any more.');
  });
});

describe('withSent and errorText', () => {
  it('adds a message it does not know and never overwrites a newer state', () => {
    const queued = msg('a', 'queued', 'a', 5);
    const sentAlready = msg('a', 'sent', 'a', 5);
    expect(withSent([], queued)).toEqual([queued]);
    expect(withSent([sentAlready], queued)).toEqual([sentAlready]);
    expect(withSent([msg('z', 'queued', 'z', 9)], msg('y', 'queued', 'y', 3)).map((m) => m.id)).toEqual(['y', 'z']);
  });

  it('reads the words of whatever was thrown', () => {
    expect(errorText('Too many waiting messages.')).toBe('Too many waiting messages.');
    expect(errorText(new Error('boom'))).toBe('boom');
    expect(errorText({})).toBe('Could not send the message.');
  });
});
