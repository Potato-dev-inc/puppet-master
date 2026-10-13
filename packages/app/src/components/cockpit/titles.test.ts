import { describe, expect, it } from 'vitest';
import { cleanTitleInput, MAX_TITLE_CHARS, multiLoginAgents, paneHeading, paneOneLine, paneSubline } from './titles';

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

describe('multiLoginAgents and paneSubline', () => {
  const work = { agent_type: 'claude', account: 'acc_work', account_label: 'Work', title: 'auth refactor' };
  const own = { agent_type: 'claude', account: null, account_label: null, title: null };

  it('flags an agent that runs under more than one login, counting the CLI own login', () => {
    expect([...multiLoginAgents([work, own])]).toEqual(['claude']);
    expect(multiLoginAgents([work, { ...work }]).size).toBe(0);
    expect(multiLoginAgents([work, { agent_type: 'codex', account: null }]).size).toBe(0);
  });

  it('puts the agent first on a titled row and adds the login only when several are in use', () => {
    const multi = new Set(['claude']);
    expect(paneSubline(work, 'Claude Code', 'Editing src/app.ts', multi)).toBe('Claude Code · Editing src/app.ts · Work');
    expect(paneSubline(work, 'Claude Code', 'Editing src/app.ts', new Set())).toBe('Claude Code · Editing src/app.ts');
  });

  it('leads with the detail on an untitled row, whose first line is already the agent', () => {
    expect(paneSubline(own, 'Claude Code', 'proj', new Set(['claude']))).toBe('proj');
    expect(paneSubline({ ...own, account_label: 'Work' }, 'Claude Code', 'proj', new Set(['claude']))).toBe('proj · Work');
  });
});
