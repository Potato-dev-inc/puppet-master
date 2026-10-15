// ONE way to say how long a card has waited, on every Inbox card: "now", "42s", "5m", "3h", "2d".
// No prefix ("asked", "waiting", "ran out") and no "ago": the card says what it is, the age says how old.

/** The short form shown at the right edge of a card. */
export function formatAge(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 5) return 'now';
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

/** The same age as a phrase, for the tooltip and screen readers: "2 minutes ago". */
export function agePhrase(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 5) return 'just now';
  const unit = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'} ago`;
  if (s < 60) return unit(s, 'second');
  const m = Math.floor(s / 60);
  if (m < 60) return unit(m, 'minute');
  const h = Math.floor(m / 60);
  if (h < 24) return unit(h, 'hour');
  return unit(Math.floor(h / 24), 'day');
}
