// Palette commands for features that otherwise hide three screens deep in Settings. Each one only
// opens the place where the feature lives (or flips Simple mode), so nothing happens by surprise.
// They open Settings on the feature's own row (a row id is a deep link, `lib/settings-target.ts`).
// This is the palette's only Simple mode command (the welcome flow adds "Welcome tour" in Palette.tsx).
// Words follow the shared word list: Live updates, Budgets, Team, Simple mode, Pair a phone.
import type { ReactNode } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { patchSettings } from '../../lib/settings';
import { IconLayers, IconNote, IconSettings, IconTerminal } from './Icons';

export interface ShortcutEntry {
  id: string;
  group: 'Actions';
  title: string;
  desc: string;
  /** Extra words that find the command (not shown). */
  keywords: string;
  icon: ReactNode;
  run: () => void;
}

export function settingsShortcuts(c: Pick<CockpitApi, 'openSettings' | 'notify'>): ShortcutEntry[] {
  const open = (tab: string) => () => void c.openSettings(tab);
  return [
    {
      id: 'act:simple-mode',
      group: 'Actions',
      title: 'Switch to Simple mode',
      desc: 'Fewer buttons and plain words. You can switch it off in Settings.',
      keywords: 'simple plain easy beginner non-technical look',
      icon: <IconLayers />,
      run: () => void patchSettings({ simple_mode: true }).catch(() => c.notify('Could not switch to Simple mode')),
    },
    {
      id: 'act:live-updates',
      group: 'Actions',
      title: 'Live updates…',
      desc: 'What Claude Code and Codex report while they work',
      keywords: 'hooks watch report trust agents',
      icon: <IconTerminal />,
      run: open('live-updates'),
    },
    {
      id: 'act:budgets',
      group: 'Actions',
      title: 'Budgets…',
      desc: 'Pause an agent after a number of turns or minutes',
      keywords: 'budget limit cost spend brake turns time',
      icon: <IconSettings />,
      run: open('budget-turns'),
    },
    {
      id: 'act:team',
      group: 'Actions',
      title: 'Team…',
      desc: 'Tell teammates when your agent pushes',
      keywords: 'team heads-up co-work teammates push labs shared',
      icon: <IconSettings />,
      run: open('team'),
    },
    {
      id: 'act:pair-phone',
      group: 'Actions',
      title: 'Pair a phone…',
      desc: 'Watch and answer your agents from your phone',
      keywords: 'phone mobile qr code pairing connect remote',
      icon: <IconSettings />,
      run: open('pairing'),
    },
    {
      id: 'act:diagnostic',
      group: 'Actions',
      title: 'Save a diagnostic file…',
      desc: 'One .zip for support. You see what is in it first.',
      keywords: 'diagnostic bug report logs support export zip help problem',
      icon: <IconNote />,
      run: open('diagnostic-file'),
    },
  ];
}
