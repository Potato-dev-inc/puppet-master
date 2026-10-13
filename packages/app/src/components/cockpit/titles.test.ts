import { describe, expect, it } from 'vitest';
import { cleanTitleInput, MAX_TITLE_CHARS, paneHeading, paneOneLine } from './titles';

describe('paneHeading', () => {
  it('uses the title as the primary line and the agent name as the secondary', () => {
    expect(paneHeading({ title: 'auth refactor' }, 'Codex')).toEqual({ primary: 'auth refactor', secondary: 'Codex' });
  });

  it('trims the title before using it', () => {
    expect(paneHeading({ title: '  auth refactor  ' }, 'Codex')).toEqual({ primary: 'auth refactor', secondary: 'Codex' });
  });

  it.each([
    ['missing', {}],
    ['null', { title: null }],
    ['empty', { title: '' }],
    ['whitespace-only', { title: '   \t ' }],
  ])('falls back to the agent name alone when the title is %s', (_label, info) => {
    expect(paneHeading(info, 'Codex')).toEqual({ primary: 'Codex', secondary: null });
  });
});

describe('paneOneLine', () => {
  it('joins the agent name and the title with a middle dot', () => {
    expect(paneOneLine({ title: 'auth refactor' }, 'Codex')).toBe('Codex \u00B7 auth refactor');
  });

  it('is just the agent name without a usable title', () => {
    expect(paneOneLine({}, 'Codex')).toBe('Codex');
    expect(paneOneLine({ title: null }, 'Codex')).toBe('Codex');
    expect(paneOneLine({ title: '   ' }, 'Codex')).toBe('Codex');
  });
});

describe('cleanTitleInput', () => {
  it('collapses runs of whitespace and trims the ends', () => {
    expect(cleanTitleInput('  auth \t\n  refactor   ')).toBe('auth refactor');
  });

  it('caps the length at MAX_TITLE_CHARS', () => {
    expect(MAX_TITLE_CHARS).toBe(40);
    expect(cleanTitleInput('x'.repeat(60))).toBe('x'.repeat(40));
  });

  it('returns an empty string for blank input, meaning clear the name', () => {
    expect(cleanTitleInput('')).toBe('');
    expect(cleanTitleInput('   \n\t ')).toBe('');
  });
});
