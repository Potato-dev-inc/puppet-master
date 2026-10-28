// Linux Inside: plain helpers for a pane's place (the chip, its tooltip, and Linux -> Windows paths).
// `toWindowsPath` is the TypeScript mirror of `wsl::path::to_windows` (spec 3.3) for the pane's own
// mount root and UNC root. It is display only: nothing here decides where an agent runs.
import type { PanePlace } from './wsl';

export const LINUX_LABEL_NAME = 'Linux';

/** The chip text, for example `Linux · Ubuntu`. Null for a Windows pane. */
export function placeLabel(place: PanePlace | null | undefined): string | null {
  if (!place || place.kind !== 'linux') return null;
  return `${LINUX_LABEL_NAME} · ${place.distro}`;
}

/** The chip tooltip: where it runs, then one line per warning. Empty for a Windows pane. */
export function placeTooltip(place: PanePlace | null | undefined): string {
  if (!place || place.kind !== 'linux') return '';
  const lines = [`Runs in ${place.distro} (WSL) in ${place.cwd}`];
  for (const warning of place.warnings ?? []) lines.push(warning);
  return lines.join('\n');
}

export type WindowsPathResult = { ok: true; path: string } | { ok: false; error: 'empty' | 'relative' };

/**
 * A Linux path as Windows writes it:
 * - `<mount_root><letter>/rest` becomes `<LETTER>:\rest` (`/mnt/d/work` -> `D:\work`);
 * - any other absolute path becomes `<unc_root>\rest` (`/home/you/p` -> `\\wsl.localhost\Ubuntu\home\you\p`).
 * A relative or empty path is refused, never guessed.
 */
export function toWindowsPath(place: PanePlace, linuxPath: string): WindowsPathResult {
  if (linuxPath === '') return { ok: false, error: 'empty' };
  if (!linuxPath.startsWith('/')) return { ok: false, error: 'relative' };

  const root = place.mount_root.endsWith('/') ? place.mount_root : `${place.mount_root}/`;
  if (linuxPath.startsWith(root)) {
    const rest = linuxPath.slice(root.length);
    const letter = rest.charAt(0);
    if (/^[A-Za-z]$/.test(letter) && (rest.length === 1 || rest.charAt(1) === '/')) {
      const tail = rest.slice(2).replace(/\//g, '\\');
      return { ok: true, path: `${letter.toUpperCase()}:\\${tail}` };
    }
  }

  const tail = linuxPath.slice(1).replace(/\//g, '\\');
  return { ok: true, path: `${place.unc_root}\\${tail}` };
}
