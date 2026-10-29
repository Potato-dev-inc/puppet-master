import type { HarnessScenario } from '../types';
import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';

// A pane that runs in Linux (WSL) shows the `Linux · <distro>` chip in its header; its tooltip lists the warnings.
const s: HarnessScenario = {
  name: 'pane-linux-chip',
  route: 'cockpit',
  select: PANE_CLAUDE,
  panes: EXAMPLE_PANES.map((p) =>
    p.id === PANE_CLAUDE
      ? {
          ...p,
          place: {
            kind: 'linux',
            distro: 'Ubuntu-Test',
            cwd: '/mnt/c/work/puppet-master',
            mount_root: '/mnt/',
            unc_root: '\\wsl.localhost\Ubuntu-Test',
            warnings: ['Line endings may differ between Windows and Linux.'],
          },
        }
      : p,
  ),
};
export default s;
