/** Paths that must not be used as a project root (macOS DMG apps often default cwd to `/`). */
export function isValidProjectPath(path: string | null | undefined): path is string {
  if (!path?.trim()) return false;
  const normalized = path.trim().replace(/\\/g, '/');
  return normalized !== '/' && !/^[A-Za-z]:$/.test(normalized);
}

/** Compare project paths after normalizing separators. */
export function projectPathsEqual(a: string, b: string): boolean {
  return a.trim().replace(/\\/g, '/') === b.trim().replace(/\\/g, '/');
}
