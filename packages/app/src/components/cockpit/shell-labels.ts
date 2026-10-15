// Words for the cockpit shell (sidebar, palette, panic sheet, Project view): a project is called by
// its folder's name, never by the id of a separate copy's folder. Pure, so it is tested on its own.
import { projectName } from '../../lib/mobile-agent-tabs';
import { projectOfCwd } from '../../lib/ship-model';

/** Where Puppet Master keeps a separate copy: `<project>/.puppet-master/worktrees/<id>`. */
const COPY_MARK = '/.puppet-master/worktrees/';

/** True when `cwd` is the folder of a separate copy (its last part is an id, not a project name). */
export function isSeparateCopyCwd(cwd: string | undefined | null): boolean {
  return !!cwd && cwd.replace(/\\/g, '/').indexOf(COPY_MARK) > 0;
}

/**
 * The project a pane works in, by name: `shop`, never the `7d4cb2b3-2-209c` folder of its separate copy.
 * Falls back to the last folder name for a path it cannot place.
 */
export function projectLabel(cwd: string | undefined | null): string {
  if (!cwd) return '';
  return projectOfCwd(cwd)?.name ?? projectName(cwd);
}

/** `shop` or `shop · separate copy`: the project name, plus a plain mark when the pane works in a copy. */
export function projectWithCopy(cwd: string | undefined | null): string {
  const name = projectLabel(cwd);
  return isSeparateCopyCwd(cwd) ? `${name} · separate copy` : name;
}
