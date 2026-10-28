import { describe, expect, it } from 'vitest';
import type { StoreCard, StoreState } from './agent-store';
import {
  chatWords,
  licenseWords,
  matchesFilter,
  primaryButton,
  searchCards,
  sortCards,
  tierWords,
} from './agent-store-model';

function card(over: Partial<StoreCard> = {}): StoreCard {
  return {
    id: 'qwen',
    label: 'Qwen Code',
    icon: 'QC',
    tagline: "Alibaba's terminal coding agent.",
    tier: 'starter',
    builtin: false,
    categories: ['open_source'],
    status: 'active',
    replaced_by: '',
    vendor: {
      name: 'Alibaba Cloud (Qwen)',
      homepage: 'https://github.com/QwenLM/qwen-code',
      license: 'Apache-2.0',
      license_kind: 'open_source',
      license_url: null,
      terms_url: null,
    },
    sign_in: { kind: 'account', summary: 'Open it and type /auth.' },
    checked: { date: '2026-10-28', version: '0.26.2', how: 'package_registry', windows_tested: false },
    state: 'available',
    state_reason: '',
    installed_version: null,
    modes: ['Pane'],
    can_install: true,
    ambiguous_name: false,
    last_check: null,
    ...over,
  };
}

describe('primaryButton (state table 3.4)', () => {
  it('available with a Windows install command shows Install', () => {
    expect(primaryButton(card({ state: 'available' }))).toEqual({ kind: 'install', label: 'Install' });
  });

  it('available without an install command shows How to install', () => {
    expect(primaryButton(card({ state: 'available', can_install: false }))).toEqual({
      kind: 'howto',
      label: 'How to install',
    });
  });

  it('found_on_pc shows Add', () => {
    expect(primaryButton(card({ state: 'found_on_pc' }))).toEqual({ kind: 'add', label: 'Add' });
  });

  it('ready shows Open', () => {
    expect(primaryButton(card({ state: 'ready' }))).toEqual({ kind: 'open', label: 'Open' });
  });

  it('added_no_program shows Install, or How to install without a command', () => {
    expect(primaryButton(card({ state: 'added_no_program' }))).toEqual({ kind: 'install', label: 'Install' });
    expect(primaryButton(card({ state: 'added_no_program', can_install: false })).kind).toBe('howto');
  });

  it('needs_review shows Review', () => {
    expect(primaryButton(card({ state: 'needs_review' }))).toEqual({ kind: 'review', label: 'Review' });
  });

  it('own_version and unsupported show no button', () => {
    expect(primaryButton(card({ state: 'own_version' }))).toEqual({ kind: 'none', label: '' });
    expect(primaryButton(card({ state: 'unsupported' }))).toEqual({ kind: 'none', label: '' });
  });

  it('covers every state', () => {
    const states: StoreState[] = [
      'available',
      'found_on_pc',
      'ready',
      'added_no_program',
      'needs_review',
      'own_version',
      'unsupported',
    ];
    for (const state of states) {
      expect(primaryButton(card({ state })).kind).toBeTruthy();
    }
  });
});

describe('matchesFilter', () => {
  const tested = card({ tier: 'verified', sign_in: { kind: 'api_key', summary: 'Paste a key.' }, state: 'ready' });
  const plain = card({ tier: 'starter', vendor: { ...card().vendor, license_kind: 'proprietary' } });

  it('all matches everything', () => {
    expect(matchesFilter(plain, 'all')).toBe(true);
    expect(matchesFilter(tested, 'all')).toBe(true);
  });

  it('ready matches only ready cards', () => {
    expect(matchesFilter(tested, 'ready')).toBe(true);
    expect(matchesFilter(plain, 'ready')).toBe(false);
  });

  it('open_source matches the open source license kind', () => {
    expect(matchesFilter(card(), 'open_source')).toBe(true);
    expect(matchesFilter(plain, 'open_source')).toBe(false);
  });

  it('needs_key matches an api_key sign-in', () => {
    expect(matchesFilter(tested, 'needs_key')).toBe(true);
    expect(matchesFilter(card(), 'needs_key')).toBe(false);
  });

  it('tested matches the verified tier only', () => {
    expect(matchesFilter(tested, 'tested')).toBe(true);
    expect(matchesFilter(plain, 'tested')).toBe(false);
  });
});

describe('searchCards', () => {
  const cards = [
    card({ id: 'qwen', label: 'Qwen Code', tagline: 'Alibaba coding agent.' }),
    card({
      id: 'kimi',
      label: 'Kimi Code',
      tagline: 'Moonshot coding agent.',
      vendor: { ...card().vendor, name: 'Moonshot AI' },
    }),
    card({
      id: 'aider',
      label: 'Aider',
      tagline: 'Pair programming in your terminal.',
      vendor: { ...card().vendor, name: 'Aider-AI' },
    }),
  ];

  it('returns every card for an empty or blank query', () => {
    expect(searchCards(cards, '')).toHaveLength(3);
    expect(searchCards(cards, '   ')).toHaveLength(3);
  });

  it('matches the label, ignoring case', () => {
    expect(searchCards(cards, 'QWEN').map((c) => c.id)).toEqual(['qwen']);
  });

  it('matches the id', () => {
    expect(searchCards(cards, 'kim').map((c) => c.id)).toEqual(['kimi']);
  });

  it('matches the tagline and the vendor name', () => {
    expect(searchCards(cards, 'terminal').map((c) => c.id)).toEqual(['aider']);
    expect(searchCards(cards, 'moonshot').map((c) => c.id)).toEqual(['kimi']);
  });

  it('needs every word of the query to match', () => {
    expect(searchCards(cards, 'kimi moonshot').map((c) => c.id)).toEqual(['kimi']);
    expect(searchCards(cards, 'kimi aider')).toEqual([]);
  });

  it('returns an empty list when nothing matches', () => {
    expect(searchCards(cards, 'zzz-nope')).toEqual([]);
  });
});

describe('plain words', () => {
  it('tier chip words', () => {
    expect(tierWords(card({ tier: 'verified' }))).toBe('Tested');
    expect(tierWords(card({ tier: 'starter' }))).toBe('Basic support');
  });

  it('license chip words', () => {
    expect(licenseWords(card())).toBe('Open source');
    expect(licenseWords(card({ vendor: { ...card().vendor, license_kind: 'source_available' } }))).toBe(
      'Source available',
    );
    expect(licenseWords(card({ vendor: { ...card().vendor, license_kind: 'proprietary' } }))).toBe('Paid terms');
  });

  it('chat words say the screen for a starter agent', () => {
    expect(chatWords(card({ tier: 'starter' }))).toBe('Chat is read from the screen.');
  });

  it('starter cards never use the words tested or verified', () => {
    const starter = card({ tier: 'starter', state: 'available', can_install: false });
    const states: StoreState[] = ['available', 'found_on_pc', 'ready', 'added_no_program', 'needs_review'];
    const texts: string[] = [tierWords(starter), licenseWords(starter), chatWords(starter)];
    for (const state of states) {
      texts.push(primaryButton({ ...starter, state }).label);
    }
    for (const text of texts) {
      expect(text.toLowerCase()).not.toContain('tested');
      expect(text.toLowerCase()).not.toContain('verified');
    }
  });
});

describe('sortCards', () => {
  it('puts ready first, then found, then available', () => {
    const sorted = sortCards([
      card({ id: 'a', label: 'A', state: 'available' }),
      card({ id: 'b', label: 'B', state: 'found_on_pc' }),
      card({ id: 'c', label: 'C', state: 'ready' }),
    ]);
    expect(sorted.map((c) => c.state)).toEqual(['ready', 'found_on_pc', 'available']);
  });

  it('puts built-in agents before others inside the same state', () => {
    const sorted = sortCards([
      card({ id: 'z', label: 'Zed', state: 'available', builtin: false }),
      card({ id: 'y', label: 'Yak', state: 'available', builtin: true }),
    ]);
    expect(sorted.map((c) => c.id)).toEqual(['y', 'z']);
  });

  it('sorts by label inside the same state and builtin flag, and does not change the input', () => {
    const input = [
      card({ id: 'b', label: 'Beta', state: 'available' }),
      card({ id: 'a', label: 'Alpha', state: 'available' }),
    ];
    const sorted = sortCards(input);
    expect(sorted.map((c) => c.label)).toEqual(['Alpha', 'Beta']);
    expect(input.map((c) => c.label)).toEqual(['Beta', 'Alpha']);
  });
});
