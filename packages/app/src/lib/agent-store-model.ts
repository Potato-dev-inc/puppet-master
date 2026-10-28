// Pure rules for the Agent Store grid: which button a card shows, which chips match, search, sort and plain words.
// No Tauri calls here, so every rule is tested with plain objects.
import type { LicenseKind, StoreCard, StoreState, StoreTier } from './agent-store';

export type PrimaryKind = 'install' | 'add' | 'open' | 'review' | 'howto' | 'none';

export interface PrimaryButton {
  kind: PrimaryKind;
  label: string;
}

/** The filter chips above the grid. */
export type StoreFilter = 'all' | 'ready' | 'open_source' | 'needs_key' | 'tested';

export const STORE_FILTERS: { id: StoreFilter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'ready', label: 'Ready' },
  { id: 'open_source', label: 'Open source' },
  { id: 'needs_key', label: 'Needs a key' },
  { id: 'tested', label: 'Tested' },
];

/**
 * The one main button for a card, from the state table of the Store design.
 * An agent with no Windows install command gets "How to install" instead of "Install".
 */
export function primaryButton(card: StoreCard): PrimaryButton {
  switch (card.state) {
    case 'available':
    case 'added_no_program':
      return card.can_install ? { kind: 'install', label: 'Install' } : { kind: 'howto', label: 'How to install' };
    case 'found_on_pc':
      return { kind: 'add', label: 'Add' };
    case 'ready':
      return { kind: 'open', label: 'Open' };
    case 'needs_review':
      return { kind: 'review', label: 'Review' };
    case 'own_version':
    case 'unsupported':
      return { kind: 'none', label: '' };
  }
}

/** True when the card passes the chip. `tested` means tier `verified`. */
export function matchesFilter(card: StoreCard, filter: StoreFilter): boolean {
  switch (filter) {
    case 'all':
      return true;
    case 'ready':
      return card.state === 'ready';
    case 'open_source':
      return card.vendor.license_kind === 'open_source';
    case 'needs_key':
      return card.sign_in.kind === 'api_key';
    case 'tested':
      return card.tier === 'verified';
  }
}

/**
 * Search by name, tagline, vendor or id. Case-insensitive.
 * Every word of the query must appear in one of those fields.
 */
export function searchCards(cards: StoreCard[], text: string): StoreCard[] {
  const words = text.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return cards;
  return cards.filter((card) => {
    const haystack = [card.label, card.tagline, card.vendor.name, card.id].join(' ').toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
}

const TIER_WORDS: Record<StoreTier, string> = {
  verified: 'Tested',
  starter: 'Basic support',
};

/** The tier chip text. */
export function tierWords(card: StoreCard): string {
  return TIER_WORDS[card.tier];
}

const LICENSE_WORDS: Record<LicenseKind, string> = {
  open_source: 'Open source',
  source_available: 'Source available',
  proprietary: 'Paid terms',
};

/** The license chip text. The card shows the vendor's wording and a link, never our reading of the license. */
export function licenseWords(card: StoreCard): string {
  return LICENSE_WORDS[card.vendor.license_kind];
}

/**
 * The line that says where Chat comes from.
 * The card does not say whether a definition declares a session file, so only the starter tier is exact.
 */
export function chatWords(card: StoreCard): string {
  if (card.tier === 'starter') return 'Chat is read from the screen.';
  return "Chat is read from the agent's session file, or from the screen.";
}

const STATE_RANK: Record<StoreState, number> = {
  ready: 0,
  added_no_program: 1,
  needs_review: 2,
  found_on_pc: 3,
  available: 4,
  own_version: 5,
  unsupported: 6,
};

/** Ready first, then found, then available. Built-in agents before others. Stable by label. */
export function sortCards(cards: StoreCard[]): StoreCard[] {
  return [...cards].sort((a, b) => {
    const byState = STATE_RANK[a.state] - STATE_RANK[b.state];
    if (byState !== 0) return byState;
    if (a.builtin !== b.builtin) return a.builtin ? -1 : 1;
    return a.label.localeCompare(b.label);
  });
}
