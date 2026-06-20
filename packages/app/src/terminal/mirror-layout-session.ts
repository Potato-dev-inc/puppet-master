/**
 * React session key for mobile mirror terminals: remount when PTY cols/rows change
 * (when {@link syncPTYResize} is false) while still resetting on agent switch (createdAt).
 */
export function mirrorLayoutSessionKey(
  createdAt: number,
  cols: number,
  rows: number,
): number {
  return createdAt + cols * 10_000 + rows;
}
