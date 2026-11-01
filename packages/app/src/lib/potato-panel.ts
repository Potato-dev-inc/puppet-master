// The right bot panel of the Potato Bot page: its tabs and the words around them. Each tab's own client code goes in
// its own file later (the Routines phase adds lib/potato-routines.ts, and so on); this file only names the tabs.

export type PanelTab = 'computer' | 'routines' | 'advanced';

export const PANEL_TABS: readonly { id: PanelTab; label: string }[] = [
  { id: 'computer', label: 'Computer' },
  { id: 'routines', label: 'Routines' },
  { id: 'advanced', label: 'Advanced' },
];

export const DEFAULT_TAB: PanelTab = 'computer';

export const PANEL_COPY = {
  show: 'Show bot panel',
  hide: 'Hide bot panel',
  close: 'Close bot panel',
  noComputer: (name: string) => `${name} has no computer here. Turn on Sandbox in Settings > General > Labs.`,
  computerOff: 'Computer is off for this bot.',
  computerOffLink: 'Turn it on in Advanced › Tools',
  noComputerThrough: 'A bot that talks through your agents has no computer of its own.',
  /** The banner in the thread while the panel is not showing the computer. */
  needsYou: (name: string) => `${name} needs you on the computer.`,
  openComputer: 'Open computer',
};
