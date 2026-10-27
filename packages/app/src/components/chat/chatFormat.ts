// Small text helpers of the chat views: durations, counts and file names. Pure code.

/** "800ms" is "<1s"; "12s"; "2m 5s". */
export function formatDuration(ms: number): string {
  if (ms < 1000) return '<1s';
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

/** "1 file", "3 files". */
export function plural(count: number, word: string): string {
  return `${count} ${count === 1 ? word : `${word}s`}`;
}

/** The last part of a path, for either slash ("a/b.ts", "a\\b.ts"). */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part !== '');
  return parts[parts.length - 1] ?? path;
}
