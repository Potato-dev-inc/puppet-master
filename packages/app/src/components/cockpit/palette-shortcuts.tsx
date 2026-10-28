// Palette commands for features that otherwise hide three screens deep in Settings. Each one only
// opens the place where the feature lives (or flips Simple mode), so nothing happens by surprise.
// They open Settings on the feature's own row (a row id is a deep link, `lib/settings-target.ts`).
// This is the palette's only Simple mode command (the welcome flow adds "Welcome tour" in Palette.tsx).
// Words follow the shared word list: Live updates, Budgets, Team, Simple mode, Pair a phone.
import type { ReactNode } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { openStartMission } from '../../lib/missions';
import { openTonight } from '../../lib/night-host';
import { patchSettings } from '../../lib/settings';
import { IconLayers, IconMoon, IconNote, IconSettings, IconTerminal } from './Icons';

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

export function settingsShortcuts(
  c: Pick<CockpitApi, 'openSettings' | 'notify'>,
  opts: { missionsOn?: boolean; nightOn?: boolean; onOpenMorning?: () => void; projectPath?: string | null } = {},
): ShortcutEntry[] {
  const open = (tab: string) => () => void c.openSettings(tab);
  // Missions (Preview): with the Labs switch on, the palette opens the start sheet (the "+" menu's
  // item); with it off, it opens the switch, so the feature can be found but nothing starts by surprise.
  const mission: ShortcutEntry = opts.missionsOn
    ? {
        id: 'act:start-mission',
        group: 'Actions',
        title: 'Start a mission…',
        desc: 'Agents work in a separate copy until your checks pass. Preview.',
        keywords: 'mission missions goal checks loop until tests pass preview labs',
        icon: <IconLayers />,
        run: () => openStartMission(opts.projectPath ?? null),
      }
    : {
        id: 'act:missions',
        group: 'Actions',
        title: 'Missions…',
        desc: 'Preview, in Labs. Turn it on to start one.',
        keywords: 'mission missions goal checks loop until tests pass preview labs',
        icon: <IconLayers />,
        run: open('missions'),
      };
  // Night Shift (Labs): hidden while the switch is off. The sidebar owns the Tonight sheet; this only asks it to open.
  const night: ShortcutEntry[] = opts.nightOn
    ? [
        {
          id: 'act:night-tonight',
          group: 'Actions',
          title: 'Night Shift: plan tonight',
          desc: 'Pick tasks for agents to work on while you sleep.',
          keywords: 'night shift tonight overnight plan tasks sleep labs',
          icon: <IconMoon />,
          run: openTonight,
        },
        ...(opts.onOpenMorning
          ? [
              {
                id: 'act:night-morning',
                group: 'Actions' as const,
                title: 'Morning',
                desc: 'See what Night Shift did, then keep or throw away each task.',
                keywords: 'morning night shift review results keep throw away overnight',
                icon: <IconMoon />,
                run: opts.onOpenMorning,
              },
            ]
          : []),
      ]
    : [];
  return [
    mission,
    ...night,
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
