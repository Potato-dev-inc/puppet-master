import { describe, expect, it } from 'vitest';
import { transcriptStreamLabel } from './transcript-labels';

// Every stream tag the labels table knows (the backend writes these; see transcript-labels.ts).
const KNOWN_TAGS = [
  'user_task',
  'user_steer',
  'user',
  'assistant_final',
  'agent',
  'result',
  'success',
  'stdout',
  'stderr',
  'permission',
  'interrupted',
  'execution_failure',
  'delivery',
  'diagnostic',
  'startup',
];

describe('transcriptStreamLabel', () => {
  it('names the known tags in plain language', () => {
    expect(transcriptStreamLabel('assistant_final')).toBe('Answer');
    expect(transcriptStreamLabel('user_task')).toBe('You');
    expect(transcriptStreamLabel('stdout')).toBe('Output');
    expect(transcriptStreamLabel('execution_failure')).toBe('Failed');
  });

  it('turns an unknown tag into sentence case', () => {
    expect(transcriptStreamLabel('some_new_tag')).toBe('Some new tag');
  });

  it('falls back to Other for an empty tag', () => {
    expect(transcriptStreamLabel('')).toBe('Other');
  });

  it('never returns an underscore for a known tag', () => {
    for (const tag of KNOWN_TAGS) {
      expect(transcriptStreamLabel(tag), tag).not.toContain('_');
    }
  });
});
