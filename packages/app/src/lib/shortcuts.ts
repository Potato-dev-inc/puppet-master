// Every keyboard shortcut the desktop app has, in one list. The sheet (components/cockpit/ShortcutsSheet.tsx)
// reads it. Each entry names the source file and a piece of code that must be there (`handlers`), and
// lib/shortcuts.test.ts fails when one of them is gone, so the list cannot drift away from the code.
// Add a shortcut here only with a real handler. `keys` are the alternatives (shown as separate keys).
// Ctrl reads as ⌘ on a Mac; `macKeys` replaces the whole list there when Ctrl stays Ctrl (terminal keys).

export type ShortcutGroup = 'Navigate' | 'Chat' | 'Panes' | 'Search';

/** The order the sheet shows the groups in. */
export const SHORTCUT_GROUPS: readonly ShortcutGroup[] = ['Navigate', 'Chat', 'Panes', 'Search'];

export interface ShortcutHandler {
  /** Path from `packages/app`. */
  file: string;
  /** Text that must appear in that file. */
  text: string;
}

export interface ShortcutEntry {
  id: string;
  group: ShortcutGroup;
  label: string;
  keys: readonly string[];
  macKeys?: readonly string[];
  handlers: readonly ShortcutHandler[];
}

const MAC_NAMES: Record<string, string> = { Ctrl: '⌘', Shift: '⇧', Alt: '⌥' };

/** True on a Mac (where Ctrl shows as ⌘). */
export function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent || '');
}

/** One key as the person sees it: `Ctrl+Shift+F` on Windows and Linux, `⌘⇧F` on a Mac. */
export function formatKey(key: string, mac: boolean): string {
  if (!mac) return key;
  return key
    .split('+')
    .map((part) => MAC_NAMES[part] ?? part)
    .join('');
}

/** The keys an entry shows on this platform. */
export function shortcutKeys(entry: ShortcutEntry, mac: boolean = isMacPlatform()): string[] {
  const keys = mac && entry.macKeys ? entry.macKeys : entry.keys;
  return keys.map((key) => formatKey(key, mac));
}

export const SHORTCUTS: readonly ShortcutEntry[] = [
  // Navigate
  {
    id: 'show-list',
    group: 'Navigate',
    label: 'Show this list',
    keys: ['?', 'Ctrl+/'],
    handlers: [
      { file: 'src/CockpitApp.tsx', text: "e.key === '?'" },
      { file: 'src/CockpitApp.tsx', text: "e.key === '/'" },
    ],
  },
  {
    id: 'close',
    group: 'Navigate',
    label: 'Close the box, sheet or menu that is open',
    keys: ['Esc'],
    handlers: [{ file: 'src/components/cockpit/Sheet.tsx', text: "event.key === 'Escape'" }],
  },
  {
    id: 'inbox-move',
    group: 'Navigate',
    label: 'Next or previous card in the Inbox',
    keys: ['J', 'K'],
    handlers: [{ file: 'src/components/cockpit/inbox-keys.ts', text: "key === 'j'" }],
  },
  {
    id: 'inbox-answer',
    group: 'Navigate',
    label: 'Answer a card in the Inbox',
    keys: ['1', '2', '3'],
    handlers: [{ file: 'src/components/cockpit/inbox-keys.ts', text: '/^[1-9]$/' }],
  },
  {
    id: 'inbox-open',
    group: 'Navigate',
    label: "Open a card's terminal in the Inbox",
    keys: ['O'],
    handlers: [{ file: 'src/components/cockpit/inbox-keys.ts', text: "key === 'o'" }],
  },
  {
    id: 'review-move',
    group: 'Navigate',
    label: 'Review: next or previous item',
    keys: ['J', 'K'],
    handlers: [{ file: 'src/components/cockpit/review/review-keys.ts', text: "lower === 'j'" }],
  },
  {
    id: 'review-open',
    group: 'Navigate',
    label: "Review: open an item's files and changes",
    keys: ['Enter', 'O'],
    handlers: [{ file: 'src/components/cockpit/review/review-keys.ts', text: "k === 'Enter' || lower === 'o'" }],
  },
  {
    id: 'review-keep',
    group: 'Navigate',
    label: 'Review: Keep the item',
    keys: ['Y'],
    handlers: [{ file: 'src/components/cockpit/review/review-keys.ts', text: "y: 'keep'" }],
  },
  {
    id: 'review-fix',
    group: 'Navigate',
    label: 'Review: Ask to fix',
    keys: ['F'],
    handlers: [{ file: 'src/components/cockpit/review/review-keys.ts', text: "f: 'fix'" }],
  },
  {
    id: 'review-throw',
    group: 'Navigate',
    label: 'Review: Throw away (you can undo for 10 seconds)',
    keys: ['X'],
    handlers: [{ file: 'src/components/cockpit/review/review-keys.ts', text: "x: 'throw'" }],
  },
  {
    id: 'review-undo',
    group: 'Navigate',
    label: 'Review: Undo the last throw away or Looks good',
    keys: ['Z'],
    handlers: [{ file: 'src/components/cockpit/review/review-keys.ts', text: "lower === 'z'" }],
  },
  {
    id: 'review-pane',
    group: 'Navigate',
    label: "Review: open the agent's pane",
    keys: ['P'],
    handlers: [{ file: 'src/components/cockpit/review/review-keys.ts', text: "p: 'open'" }],
  },
  {
    id: 'review-close',
    group: 'Navigate',
    label: 'Review: close the item, then the Review Desk',
    keys: ['Esc'],
    handlers: [{ file: 'src/components/cockpit/review/review-keys.ts', text: "k === 'Escape'" }],
  },
  {
    id: 'save-memory',
    group: 'Navigate',
    label: 'Save project memory',
    keys: ['Ctrl+S'],
    handlers: [{ file: 'src/components/cockpit/ProjectMemory.tsx', text: "e.key.toLowerCase() === 's'" }],
  },

  // Chat
  {
    id: 'send',
    group: 'Chat',
    label: 'Send your message',
    keys: ['Enter'],
    handlers: [{ file: 'src/components/chat/ChatComposer.tsx', text: "event.key !== 'Enter' || event.shiftKey" }],
  },
  {
    id: 'new-line',
    group: 'Chat',
    label: 'New line in your message',
    keys: ['Shift+Enter'],
    handlers: [{ file: 'src/components/chat/ChatComposer.tsx', text: "event.key !== 'Enter' || event.shiftKey" }],
  },
  {
    id: 'ask-aside',
    group: 'Chat',
    label: 'Ask aside: a side question that does not write into the chat',
    keys: ['Ctrl+;'],
    handlers: [{ file: 'src/components/chat/useSideQuestion.tsx', text: "event.key === ';'" }],
  },
  {
    id: 'remember',
    group: 'Chat',
    label: 'Remember the selected text for all agents',
    keys: ['Ctrl+Shift+M'],
    handlers: [{ file: 'src/components/chat/RememberSelection.tsx', text: "e.key.toLowerCase() === 'm'" }],
  },
  {
    id: 'ask-bar',
    group: 'Chat',
    label: 'Ask bar: a quick question to an agent, from any app (set in Settings)',
    keys: ['Ctrl+Alt+Space'],
    handlers: [{ file: 'src-tauri/src/askbar/mod.rs', text: 'DEFAULT_SHORTCUT: &str = "Ctrl+Alt+Space"' }],
  },

  // Panes
  {
    id: 'split',
    group: 'Panes',
    label: 'Split the view with the next pane. Again closes the split',
    keys: ['Ctrl+\\'],
    handlers: [{ file: 'src/components/cockpit/main-view.ts', text: "e.key === '\\\\'" }],
  },
  {
    id: 'rail',
    group: 'Panes',
    label: 'Show or hide the side rail',
    keys: [']'],
    handlers: [{ file: 'src/components/cockpit/rail/useRailLayout.ts', text: "e.key !== ']'" }],
  },
  {
    id: 'interrupt',
    group: 'Panes',
    label: 'Stop what the agent is doing (sends Ctrl+C to the focused pane)',
    keys: ['Ctrl+C'],
    macKeys: ['⌃C'],
    handlers: [{ file: 'src/terminal/canvas-terminal.ts', text: "event.key === 'c' && event.ctrlKey" }],
  },
  {
    id: 'paste',
    group: 'Panes',
    label: 'Paste into the focused pane',
    keys: ['Ctrl+V'],
    handlers: [{ file: 'src/terminal/xterm-textarea.ts', text: "event.key.toLowerCase() === 'v'" }],
  },
  {
    id: 'stop-all',
    group: 'Panes',
    label: 'Stop all agents, from any app (turn on in Settings)',
    keys: ['Ctrl+Alt+Shift+P'],
    handlers: [{ file: 'src-tauri/src/panic.rs', text: '"Ctrl+Alt+Shift+P"' }],
  },

  // Search
  {
    id: 'palette',
    group: 'Search',
    label: 'Open the command palette: switch panes, start an agent, run an action',
    keys: ['Ctrl+K'],
    handlers: [{ file: 'src/CockpitApp.tsx', text: "key === 'k'" }],
  },
  {
    id: 'search-chats',
    group: 'Search',
    label: 'Search what was said in all chats',
    keys: ['Ctrl+Shift+F'],
    handlers: [{ file: 'src/CockpitApp.tsx', text: "key === 'f'" }],
  },
  {
    id: 'palette-move',
    group: 'Search',
    label: 'Move up and down the palette list',
    keys: ['↑', '↓'],
    handlers: [{ file: 'src/components/cockpit/Palette.tsx', text: "e.key === 'ArrowDown'" }],
  },
  {
    id: 'palette-open',
    group: 'Search',
    label: 'Open the highlighted row',
    keys: ['Enter'],
    handlers: [{ file: 'src/components/cockpit/Palette.tsx', text: "e.key === 'Enter'" }],
  },
  {
    id: 'palette-back',
    group: 'Search',
    label: 'In chat search with an empty box: back to the commands',
    keys: ['Backspace'],
    handlers: [{ file: 'src/components/cockpit/Palette.tsx', text: "e.key === 'Backspace' && mode === 'chats'" }],
  },
];
