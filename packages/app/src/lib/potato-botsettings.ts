// The per-bot settings of Potato Bot, on the bot panel's Advanced tab: the bot's own talk brain (or the default), its
// instructions with their history, its three tool switches, Rename, Delete and "Export chat". The names mirror
// src-tauri/src/potato/bot_settings.rs and src-tauri/src/commands/potato_bot_settings.rs, in camelCase. Desktop only.
// `fakeBotSettingsTransport` is the in-memory twin for tests.
import { invoke } from '@tauri-apps/api/core';
import { chatExportSave } from './chatExport';
import type { BrainKind } from './potato';

/** The three tool switches of a bot. All on by default. */
export interface ToolSwitches {
  /** Local helper agents (`ask_helper`). */
  helpers: boolean;
  /** The sandbox screen tools. Shown only when the sandbox Labs switch is on. */
  computer: boolean;
  /** Reading the web in the sandbox browser. */
  web: boolean;
}

export type ToolName = keyof ToolSwitches;

export interface InstructionVersion {
  /** When it was saved (epoch milliseconds). The key Restore uses. */
  at: number;
  text: string;
}

/** What the Advanced tab reads. `versions` are newest first. `brain` null is "Use the default". */
export interface BotOptions {
  brain: BrainKind | null;
  tools: ToolSwitches;
  sandboxOn: boolean;
  instructions: string;
  versions: InstructionVersion[];
}

/** A chat as Markdown (the text, for the preview) and the one-time token that saves exactly that text. */
export interface BotExport {
  markdown: string;
  token: string;
}

export const INSTRUCTIONS_MAX = 4000;
/** Above this many characters the counter shows, so the limit is never a surprise. */
export const INSTRUCTIONS_COUNT_FROM = 3000;

export interface BotSettingsTransport {
  options(botId: string): Promise<BotOptions>;
  /** `null` goes back to the default brain. */
  setBrain(botId: string, brain: BrainKind | null): Promise<BotOptions>;
  setTools(botId: string, tools: ToolSwitches): Promise<BotOptions>;
  saveInstructions(botId: string, text: string): Promise<BotOptions>;
  restoreInstructions(botId: string, at: number): Promise<BotOptions>;
  rename(botId: string, name: string): Promise<void>;
  /** The name must be typed exactly; the engine checks it again. */
  remove(botId: string, typedName: string): Promise<void>;
  exportChat(botId: string): Promise<BotExport>;
  /** Writes the export behind `token` to `path` (what the save dialog returned). Resolves to the path written. */
  saveExport(token: string, path: string): Promise<string>;
}

export const tauriBotSettingsTransport: BotSettingsTransport = {
  options: (botId) => invoke<BotOptions>('potato_bot_options_get', { botId }),
  setBrain: (botId, brain) => invoke<BotOptions>('potato_bot_brain_set', { botId, brain }),
  setTools: (botId, tools) => invoke<BotOptions>('potato_bot_tools_set', { botId, tools }),
  saveInstructions: (botId, text) => invoke<BotOptions>('potato_bot_instructions_save', { botId, text }),
  restoreInstructions: (botId, at) => invoke<BotOptions>('potato_bot_instructions_restore', { botId, at }),
  rename: async (botId, name) => {
    await invoke('potato_bot_rename', { botId, name });
  },
  remove: async (botId, typedName) => {
    await invoke('potato_bot_delete', { botId, typedName });
  },
  exportChat: (botId) => invoke<BotExport>('potato_bot_export_markdown', { botId }),
  saveExport: chatExportSave,
};

// ---- copy and small rules -----------------------------------------------------------------------

export const BRAIN_CHOICE_LABEL: Record<BrainKind, string> = { deepseek: 'DeepSeek', go: 'OpenCode Go', agents: 'Through my agents' };

/** The choices of the Model row, in order. The first is the default; its label names the default brain. */
export function brainChoices(defaultBrain: BrainKind): { value: BrainKind | 'default'; label: string }[] {
  return [
    { value: 'default', label: `Use the default (${BRAIN_CHOICE_LABEL[defaultBrain]})` },
    { value: 'deepseek', label: BRAIN_CHOICE_LABEL.deepseek },
    { value: 'go', label: BRAIN_CHOICE_LABEL.go },
    { value: 'agents', label: BRAIN_CHOICE_LABEL.agents },
  ];
}

/** The Model row's value for a saved choice. */
export function brainValue(brain: BrainKind | null): BrainKind | 'default' {
  return brain ?? 'default';
}

/** The row's value back to what is saved: `default` is `null`. */
export function brainFromValue(value: string): BrainKind | null {
  return value === 'deepseek' || value === 'go' || value === 'agents' ? value : null;
}

export const TOOL_ROWS: readonly { id: ToolName; label: string; line: string }[] = [
  { id: 'helpers', label: 'Helpers', line: 'Start local agents to do work on this computer. You still press Start.' },
  { id: 'computer', label: 'Computer', line: "Let the bot use its sandbox computer and its screen." },
  { id: 'web', label: 'Web reading', line: 'Let the bot open and read web pages in the sandbox browser.' },
];

/** The tool rows this bot can show: the computer row only when the sandbox Labs switch is on. */
export function toolRows(sandboxOn: boolean): typeof TOOL_ROWS {
  return TOOL_ROWS.filter((row) => row.id !== 'computer' || sandboxOn);
}

/** Typing the name is the only way to delete. Spaces at the ends do not count. */
export function nameTypedRight(typed: string, name: string): boolean {
  return name.trim() !== '' && typed.trim() === name.trim();
}

/** `Nov 1, 3:10 PM`: the date and the clock of one saved version. */
export function versionWhen(at: number): string {
  return new Date(at).toLocaleString('en-US', { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
}

/** The first line of a version, cut to 80 characters: the row's preview. */
export function versionPreview(text: string): string {
  const first = text.split('\n').find((line) => line.trim() !== '') ?? '';
  return first.length > 80 ? `${first.slice(0, 79)}…` : first;
}

export const ADVANCED_COPY = {
  name: 'Name',
  nameLabel: 'Bot name',
  saveName: 'Save name',
  model: 'Model',
  modelLine: 'The brain that talks for this bot. The change applies from its next message.',
  instructions: 'Instructions',
  instructionsPlaceholder: 'Extra things this bot should always keep in mind.',
  saveInstructions: 'Save',
  saved: 'Saved.',
  restored: 'Restored.',
  inUse: 'In use',
  restore: 'Restore',
  show: 'Show',
  hide: 'Hide',
  history: 'History',
  noHistory: 'No saved versions yet.',
  instructionsLine: 'Takes effect the next time the bot starts. Every save keeps an older version.',
  tools: 'Tools',
  toolsLine: 'Turn off what this bot may do. A switch that is off refuses the tool at once.',
  bot: 'Bot',
  exportChat: 'Export chat',
  exporting: 'Saving…',
  exported: (path: string) => `Saved the chat to ${path}.`,
  remove: 'Delete bot',
  removeTitle: (name: string) => `Delete ${name}?`,
  removeLine: "Its chat and brain memory move to the removed folder. Its companion stays, so you can make it a bot again.",
  typeToConfirm: (name: string) => `Type ${name} to confirm`,
  cancel: 'Cancel',
  deleteNow: 'Delete',
  renamed: 'Name saved.',
};

// ---- the fake -----------------------------------------------------------------------------------

export interface FakeBotSettingsSeed {
  /** What each bot starts with. */
  options?: Partial<BotOptions>;
  /** Whether the sandbox Labs switch is on. */
  sandboxOn?: boolean;
  /** Makes the next call of this kind fail with this sentence. */
  fail?: Partial<Record<keyof BotSettingsTransport, string>>;
  /** What exportChat answers. */
  markdown?: string;
  /** The bot's name: a delete succeeds only when the typed name matches it. Default: `Nimbus`. */
  name?: string;
  /** The clock for saved versions (epoch milliseconds). */
  now?: () => number;
}

export interface FakeBotSettingsTransport extends BotSettingsTransport {
  readonly calls: { method: keyof BotSettingsTransport; args: unknown[] }[];
}

export function fakeBotSettingsTransport(seed: FakeBotSettingsSeed = {}): FakeBotSettingsTransport {
  const calls: FakeBotSettingsTransport['calls'] = [];
  let state: BotOptions = {
    brain: null,
    tools: { helpers: true, computer: true, web: true },
    sandboxOn: seed.sandboxOn ?? true,
    instructions: '',
    versions: [],
    ...seed.options,
  };
  const now = seed.now ?? (() => 1_700_000_000_000);
  const run = <T>(method: keyof BotSettingsTransport, args: unknown[], change: () => T): Promise<T> => {
    calls.push({ method, args });
    const message = seed.fail?.[method];
    if (message) return Promise.reject(new Error(message));
    try {
      return Promise.resolve(change());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const snapshot = (): BotOptions => ({ ...state, tools: { ...state.tools }, versions: state.versions.map((v) => ({ ...v })) });
  const saveText = (text: string) => {
    const clean = text.trim();
    if (clean.length > INSTRUCTIONS_MAX) throw new Error(`Instructions can be ${INSTRUCTIONS_MAX} characters at most.`);
    if (clean === state.instructions) return;
    state.instructions = clean;
    const at = Math.max(now(), (state.versions[0]?.at ?? 0) + 1);
    state.versions = [{ at, text: clean }, ...state.versions];
  };
  return {
    calls,
    options: (botId) => run('options', [botId], snapshot),
    setBrain: (botId, brain) =>
      run('setBrain', [botId, brain], () => {
        state.brain = brain;
        return snapshot();
      }),
    setTools: (botId, tools) =>
      run('setTools', [botId, tools], () => {
        state.tools = { ...tools };
        return snapshot();
      }),
    saveInstructions: (botId, text) =>
      run('saveInstructions', [botId, text], () => {
        saveText(text);
        return snapshot();
      }),
    restoreInstructions: (botId, at) =>
      run('restoreInstructions', [botId, at], () => {
        const version = state.versions.find((v) => v.at === at);
        if (!version) throw new Error('That version of the instructions is gone.');
        saveText(version.text);
        return snapshot();
      }),
    rename: (botId, name) => run('rename', [botId, name], () => undefined),
    remove: (botId, typedName) =>
      run('remove', [botId, typedName], () => {
        if (!nameTypedRight(typedName, seed.name ?? 'Nimbus')) throw new Error("Type the bot's name exactly to delete it.");
      }),
    exportChat: (botId) => run('exportChat', [botId], () => ({ markdown: seed.markdown ?? '# Chat', token: 'tok-fake' })),
    saveExport: (token, path) => run('saveExport', [token, path], () => path),
  };
}
