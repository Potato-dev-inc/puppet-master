// Potato Bot's sandbox on the desktop: a small private Linux computer the bot can use. The names mirror the Rust serde shapes in
// src-tauri/src/potato/sandbox/{manager,detect}.rs and src-tauri/src/commands/potato_sandbox.rs, in camelCase. Desktop only: the page
// talks to Tauri commands and to two events; the phone and the bridge never see this. Nothing here draws.
// `fakeSandboxTransport` is the in-memory twin for tests (the pattern of `fakePotatoTransport`).
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauri } from './cockpit-api';

export type SandboxState = 'not_installed' | 'installing' | 'incomplete' | 'stopped' | 'starting' | 'running' | 'failed';
export type InstallState = 'idle' | 'running' | 'done' | 'failed' | 'cancelled';

export interface InstallView {
  state: InstallState;
  /** 0 to 100 over the whole set up. */
  percent: number;
  step: string | null;
  /** The step's plain name ("Downloading Ubuntu"). */
  title: string | null;
  index: number;
  of: number;
  /** While the recipes run: the line the installer printed. */
  detail: string | null;
  cancelling: boolean;
  error: { code: string; message: string } | null;
  /** Set up stopped because Windows' Linux feature is missing: the page shows the button for it. */
  needsLinuxFeature: boolean;
}

export interface SandboxView {
  state: SandboxState;
  reason: string | null;
  name: string;
  idleMinutes: number;
  /** Windows' Linux feature answers. */
  linuxFeature: boolean;
  diskBytes: number | null;
  hasResetPoint: boolean;
  resetPointBytes: number | null;
  /** State `incomplete` (set up did not finish) and the answers it started with are known: "Continue setup" works. Else only "Start over". */
  canContinue: boolean;
  install: InstallView;
}

export interface RecipeChoice {
  id: string;
  title: string;
  description: string;
}

export interface RecipeOption {
  id: string;
  title: string;
  description: string;
  kind: 'toggle' | 'single' | 'multi';
  /** A bool, a choice id, or a list of choice ids. */
  default: unknown;
  choices: RecipeChoice[];
}

export interface Recipe {
  id: string;
  title: string;
  description: string;
  exclusive: string | null;
  requires: string | null;
  options: RecipeOption[];
}

/** What the person's projects use (marker files like package.json or Cargo.toml). */
export interface Detected {
  languages: string[];
  packageManager: string | null;
  projects: number;
}

export type Look = 'ubuntu' | 'zorin' | 'light';
export type Use = 'coding' | 'daily';

/** What the setup wizard collected (`WizardAnswers` in manager.rs). */
export interface WizardAnswers {
  look: Look;
  uses: Use[];
  /** `recipe.option` -> value; a list is the ids joined by commas. */
  options: Record<string, string>;
  resetPoint: boolean;
}

export interface FeatureAsk {
  token: string;
  title: string;
  body: string;
}

export interface SandboxTransport {
  status(): Promise<SandboxView>;
  menu(): Promise<Recipe[]>;
  detect(): Promise<Detected>;
  /** Starts set up and returns at once; follow `onInstall`. A bad answer rejects with a plain sentence. */
  install(answers: WizardAnswers): Promise<void>;
  /** Carries on a set up that did not finish (state `incomplete`), with the answers it started with. Follow `onInstall`. */
  continueInstall(): Promise<void>;
  cancelInstall(): Promise<void>;
  /** The page showed the end of a set up: forget it. */
  dismissInstall(): Promise<void>;
  start(): Promise<void>;
  stop(): Promise<void>;
  /** Back to the reset point saved at the end of set up. */
  reset(): Promise<void>;
  /** `typed` must be the word `remove`. */
  remove(typed: string): Promise<void>;
  /** Returns the number that is used (the app keeps it between 1 and 240). */
  setIdle(minutes: number): Promise<number>;
  /** Step one of "turn on Windows' Linux feature": the text of the confirm dialog and a token for its button. */
  linuxAsk(): Promise<FeatureAsk>;
  /** Step two: the person pressed the dialog's button. */
  linuxEnable(token: string): Promise<{ restartNeeded: boolean }>;
  onInstall?(cb: (view: InstallView) => void): Promise<UnlistenFn>;
  onChanged?(cb: () => void): Promise<UnlistenFn>;
}

export const tauriSandboxTransport: SandboxTransport = {
  status: () => invoke<SandboxView>('potato_sandbox_status'),
  menu: () => invoke<Recipe[]>('potato_sandbox_menu'),
  detect: () => invoke<Detected>('potato_sandbox_detect'),
  install: (answers) => invoke<void>('potato_sandbox_install', { answers }),
  continueInstall: () => invoke<void>('potato_sandbox_continue_install'),
  cancelInstall: () => invoke<void>('potato_sandbox_cancel_install'),
  dismissInstall: () => invoke<void>('potato_sandbox_dismiss_install'),
  start: () => invoke<void>('potato_sandbox_start'),
  stop: () => invoke<void>('potato_sandbox_stop'),
  reset: () => invoke<void>('potato_sandbox_reset'),
  remove: (typed) => invoke<void>('potato_sandbox_remove', { typed }),
  setIdle: (minutes) => invoke<number>('potato_sandbox_set_idle', { minutes }),
  linuxAsk: () => invoke<FeatureAsk>('potato_sandbox_linux_ask'),
  linuxEnable: (token) => invoke<{ restartNeeded: boolean }>('potato_sandbox_linux_enable', { token }),
  onInstall: (cb) => {
    if (!isTauri()) return Promise.resolve((() => {}) as UnlistenFn);
    return listen<{ install: InstallView }>('potato://sandbox-install', (e) => cb(e.payload.install));
  },
  onChanged: (cb) => {
    if (!isTauri()) return Promise.resolve((() => {}) as UnlistenFn);
    return listen('potato://sandbox-changed', () => cb());
  },
};

// ---- copy and small helpers -------------------------------------------------------------------------

export const STATE_WORD: Record<SandboxState, string> = {
  not_installed: 'Not set up',
  installing: 'Setting up',
  incomplete: "Setup didn't finish",
  stopped: 'Stopped',
  starting: 'Starting',
  running: 'Running',
  failed: 'Could not start',
};

export const LOOK_LABEL: Record<Look, string> = {
  ubuntu: 'Ubuntu',
  zorin: 'Ubuntu with the Zorin look',
  light: 'Light desktop',
};

export const USE_LABEL: Record<Use, string> = { coding: 'Coding', daily: 'Daily use' };

/** What is always in the sandbox, whatever is picked (spec: the founder's "always" list). */
export const ALWAYS_INCLUDED = 'Chrome, a terminal, a file manager, a text editor, a PDF viewer, zip, emoji and fonts for every language, copy and paste, and screenshots.';

export const SIGN_IN_NOTE = "Sign-ins for your apps (Gmail, Calendar, Drive and so on) happen inside the sandbox's Chrome. Your sign-ins on this computer are not copied into it.";

export const REMOVE_WORD = 'remove';

/** `3.7 GB`, `640 MB`. */
export function formatSize(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = bytes / 1024 ** 2;
  return `${Math.max(1, Math.round(mb))} MB`;
}

export function formatGb(gb: number): string {
  return gb < 1 ? `${Math.round(gb * 1000 / 10) * 10} MB` : `${gb.toFixed(1)} GB`;
}

// ---- the estimate (numbers from the sandbox spike; the ones marked `rough` were not measured) ----------

export interface Estimate {
  downloadGb: number;
  diskGb: number;
  /** The reset point (a copy of the finished sandbox) is extra. 0 when it is off. */
  resetPointGb: number;
  minutes: [number, number];
}

interface Cost {
  down: number;
  disk: number;
  /** minutes on a fast connection */
  min: number;
}

// Measured on the spike PC (RESULTS.md): the XFCE desktop and Chrome came to 324 MB of downloads, 3.7 GB for the disk file
// and about 6 minutes; the Zorin look was 83 MB, 393 MB and 86 seconds; the reset point was 2.99 GB for that 3.7 GB disk file.
const BASE: Cost = { down: 0.7, disk: 3.7, min: 8 }; // image (rough: 0.35 GB), desktop, Chrome
const LIGHT_SAVES: Cost = { down: 0.05, disk: 0.3, min: 0 }; // rough
const ZORIN: Cost = { down: 0.083, disk: 0.393, min: 2 }; // measured
const CODING: Cost = { down: 0.25, disk: 0.8, min: 3 }; // rough: git, gh, Node.js, compilers
const DAILY: Cost = { down: 0.35, disk: 1.1, min: 4 }; // rough: LibreOffice
const LANGUAGE: Record<string, Cost> = {
  python: { down: 0.05, disk: 0.2, min: 1 },
  go: { down: 0.1, disk: 0.5, min: 1 },
  rust: { down: 0.15, disk: 0.8, min: 2 },
  java: { down: 0.2, disk: 0.4, min: 1 },
  dotnet: { down: 0.15, disk: 0.6, min: 2 },
};
const EXTRA: Cost = { down: 0.02, disk: 0.06, min: 0 }; // rough, per extra tool
const EXTRA_BIG: Record<string, Cost> = { ffmpeg: { down: 0.1, disk: 0.3, min: 1 } };
const MEDIA: Record<string, Cost> = { vlc: { down: 0.05, disk: 0.2, min: 1 }, mpv: { down: 0.02, disk: 0.08, min: 0 } };
const IMAGES: Cost = { down: 0.02, disk: 0.1, min: 0 };
const RESET_POINT_RATIO = 0.8; // 2.99 / 3.7

const list = (value: string | undefined): string[] => (value ? value.split(',').filter(Boolean) : []);

/** How much a set up will download, take on disk and last. Always shown as "about". */
export function estimateFor(answers: WizardAnswers): Estimate {
  let down = BASE.down;
  let disk = BASE.disk;
  let min = BASE.min;
  const add = (c: Cost, times = 1) => {
    down += c.down * times;
    disk += c.disk * times;
    min += c.min * times;
  };
  if (answers.look === 'light') {
    down -= LIGHT_SAVES.down;
    disk -= LIGHT_SAVES.disk;
  }
  if (answers.look === 'zorin') add(ZORIN);
  if (answers.uses.includes('coding')) {
    add(CODING);
    for (const language of list(answers.options['coding.languages'])) add(LANGUAGE[language] ?? EXTRA);
    for (const extra of list(answers.options['coding.extras'])) add(EXTRA_BIG[extra] ?? EXTRA);
  }
  if (answers.uses.includes('daily')) {
    add(DAILY);
    const media = answers.options['daily.media'];
    if (media && MEDIA[media]) add(MEDIA[media]);
    const images = answers.options['daily.images'];
    if (images && images !== 'none') add(IMAGES);
  }
  const round = (n: number) => Math.round(n * 10) / 10;
  return {
    downloadGb: round(down),
    diskGb: round(disk),
    resetPointGb: answers.resetPoint ? round(disk * RESET_POINT_RATIO) : 0,
    minutes: [Math.round(min), Math.round(min * 2.5)],
  };
}

// ---- the wizard's draft and its defaults -------------------------------------------------------------

/** What the person has picked so far. */
export interface Draft {
  look: Look;
  coding: boolean;
  daily: boolean;
  languages: string[];
  packageManager: string;
  extras: string[];
  media: string;
  images: string;
  resetPoint: boolean;
}

const optionOf = (menu: Recipe[], recipe: string, option: string): RecipeOption | undefined =>
  menu.find((r) => r.id === recipe)?.options.find((o) => o.id === option);

const singleDefault = (option: RecipeOption | undefined, fallback: string): string => {
  const wanted = typeof option?.default === 'string' ? option.default : fallback;
  return option?.choices.some((c) => c.id === wanted) ? wanted : (option?.choices[0]?.id ?? fallback);
};

/**
 * The first draft: Ubuntu, coding, the languages the projects use already checked, the package manager they use, and the
 * recipe's own defaults for the rest.
 */
export function initialDraft(menu: Recipe[], detected: Detected | null): Draft {
  const languages = optionOf(menu, 'coding', 'languages');
  const pkg = optionOf(menu, 'coding', 'pkg');
  const known = new Set(languages?.choices.map((c) => c.id) ?? []);
  const found = (detected?.languages ?? []).filter((id) => known.has(id));
  const usedPm = detected?.packageManager && pkg?.choices.some((c) => c.id === detected.packageManager) ? detected.packageManager : null;
  return {
    look: 'ubuntu',
    coding: true,
    daily: false,
    languages: found,
    packageManager: usedPm ?? singleDefault(pkg, 'npm'),
    extras: Array.isArray(optionOf(menu, 'coding', 'extras')?.default) ? (optionOf(menu, 'coding', 'extras')!.default as string[]) : [],
    media: singleDefault(optionOf(menu, 'daily', 'media'), 'vlc'),
    images: singleDefault(optionOf(menu, 'daily', 'images'), 'eog'),
    resetPoint: true,
  };
}

/** The draft as the answers the app takes. Choices of a use that is not picked are left out. */
export function answersOf(draft: Draft): WizardAnswers {
  const uses: Use[] = [];
  const options: Record<string, string> = {};
  if (draft.coding) {
    uses.push('coding');
    options['coding.pkg'] = draft.packageManager;
    if (draft.languages.length > 0) options['coding.languages'] = draft.languages.join(',');
    if (draft.extras.length > 0) options['coding.extras'] = draft.extras.join(',');
  }
  if (draft.daily) {
    uses.push('daily');
    options['daily.media'] = draft.media;
    options['daily.images'] = draft.images;
  }
  return { look: draft.look, uses, options, resetPoint: draft.resetPoint };
}

/** `Python, Go and Rust`. */
export function joinWords(words: string[]): string {
  if (words.length <= 1) return words.join('');
  return `${words.slice(0, -1).join(', ')} and ${words[words.length - 1]}`;
}

/** `CODE: A plain sentence.` from a Tauri command, shown as the sentence only. */
export function plainSandboxError(error: unknown, fallback: string): string {
  const raw = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  const text = raw.replace(/^[A-Z][A-Z0-9_]{2,}:\s*/, '').trim();
  return text || fallback;
}

// ---- the fake -----------------------------------------------------------------------------------------

export function emptyInstall(): InstallView {
  return { state: 'idle', percent: 0, step: null, title: null, index: 0, of: 0, detail: null, cancelling: false, error: null, needsLinuxFeature: false };
}

export const FAKE_MENU: Recipe[] = [
  {
    id: 'coding',
    title: 'Coding tools',
    description: 'Tools to clone, build and test code.',
    exclusive: null,
    requires: null,
    options: [
      {
        id: 'pkg',
        title: 'Package manager',
        description: 'The tool that installs JavaScript packages.',
        kind: 'single',
        default: 'npm',
        choices: [
          { id: 'npm', title: 'npm', description: 'Comes with Node.js.' },
          { id: 'pnpm', title: 'pnpm', description: 'Fast installs that share files between projects.' },
          { id: 'yarn', title: 'Yarn', description: 'Yarn classic, installed globally.' },
          { id: 'bun', title: 'Bun', description: 'A fast JavaScript runtime and package manager.' },
        ],
      },
      {
        id: 'languages',
        title: 'Languages',
        description: 'Extra programming languages.',
        kind: 'multi',
        default: [],
        choices: [
          { id: 'python', title: 'Python', description: 'Python 3 with pip and venv.' },
          { id: 'go', title: 'Go', description: 'The Go toolchain.' },
          { id: 'rust', title: 'Rust', description: 'Cargo and rustc.' },
          { id: 'java', title: 'Java', description: 'OpenJDK 21.' },
          { id: 'dotnet', title: '.NET', description: '.NET SDK 8.' },
        ],
      },
      {
        id: 'extras',
        title: 'Extras',
        description: 'Small tools that many projects use.',
        kind: 'multi',
        default: [],
        choices: [
          { id: 'tmux', title: 'tmux', description: 'Several terminal panes in one window.' },
          { id: 'sqlite', title: 'SQLite', description: 'The sqlite3 command line.' },
          { id: 'ffmpeg', title: 'FFmpeg', description: 'Converts audio and video.' },
        ],
      },
    ],
  },
  {
    id: 'daily',
    title: 'Everyday apps',
    description: 'LibreOffice, a media player and an image viewer.',
    exclusive: null,
    requires: null,
    options: [
      {
        id: 'media',
        title: 'Media player',
        description: 'Plays audio and video.',
        kind: 'single',
        default: 'vlc',
        choices: [
          { id: 'vlc', title: 'VLC', description: 'Plays almost every format.' },
          { id: 'mpv', title: 'mpv', description: 'Small and fast.' },
          { id: 'none', title: 'No media player', description: 'Do not install one.' },
        ],
      },
      {
        id: 'images',
        title: 'Image viewer',
        description: 'Opens pictures and screenshots.',
        kind: 'single',
        default: 'eog',
        choices: [
          { id: 'eog', title: 'Image Viewer', description: 'The simple GNOME viewer.' },
          { id: 'gthumb', title: 'gThumb', description: 'Viewer with a folder browser.' },
          { id: 'none', title: 'No image viewer', description: 'Do not install one.' },
        ],
      },
    ],
  },
];

export interface FakeSandboxSeed {
  status?: Partial<SandboxView>;
  menu?: Recipe[];
  detected?: Detected;
  /** Reject `install` with this sentence. */
  installError?: string;
  /** Reject `detect`. */
  detectFails?: boolean;
}

export interface FakeSandboxTransport extends SandboxTransport {
  readonly calls: { method: keyof SandboxTransport; args: unknown[] }[];
  /** The status the app would answer now. */
  readonly view: SandboxView;
  /** Pushes an install event, as the app does while a set up runs. */
  pushInstall(patch: Partial<InstallView>): void;
  /** The set up ended well. */
  finishInstall(): void;
  /** The set up stopped with this sentence. */
  failInstall(message: string, over?: Partial<InstallView>): void;
  /** Changes the status and fires the `changed` listeners (an idle stop, a start from somewhere else). */
  patch(patch: Partial<SandboxView>): void;
}

export function fakeSandboxTransport(seed: FakeSandboxSeed = {}): FakeSandboxTransport {
  const view: SandboxView = {
    state: 'not_installed',
    reason: null,
    name: 'sandbox',
    idleMinutes: 15,
    linuxFeature: true,
    diskBytes: null,
    hasResetPoint: false,
    resetPointBytes: null,
    canContinue: false,
    install: emptyInstall(),
    ...seed.status,
  };
  const calls: FakeSandboxTransport['calls'] = [];
  const installListeners = new Set<(v: InstallView) => void>();
  const changedListeners = new Set<() => void>();
  const log = <T,>(method: keyof SandboxTransport, args: unknown[], run: () => T): Promise<T> => {
    calls.push({ method, args });
    try {
      return Promise.resolve(run());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const emitInstall = () => installListeners.forEach((cb) => cb({ ...view.install }));
  const emitChanged = () => changedListeners.forEach((cb) => cb());
  return {
    calls,
    get view() {
      return view;
    },
    pushInstall(patch) {
      view.install = { ...view.install, state: 'running', ...patch };
      view.state = 'installing';
      emitInstall();
    },
    finishInstall() {
      view.install = { ...view.install, state: 'done', percent: 100, cancelling: false, error: null };
      view.state = 'stopped';
      view.hasResetPoint = true;
      view.diskBytes = 3.7 * 1024 ** 3;
      emitInstall();
      emitChanged();
    },
    failInstall(message, over = {}) {
      view.install = { ...view.install, state: 'failed', cancelling: false, error: { code: 'SANDBOX_INSTALL_FAILED', message }, ...over };
      view.state = 'not_installed';
      emitInstall();
      emitChanged();
    },
    patch(patch) {
      Object.assign(view, patch);
      emitChanged();
    },
    status: () => log('status', [], () => ({ ...view, install: { ...view.install } })),
    menu: () => log('menu', [], () => (seed.menu ?? FAKE_MENU).map((r) => ({ ...r }))),
    detect: () =>
      log('detect', [], () => {
        if (seed.detectFails) throw new Error('Could not look at your projects.');
        return { ...(seed.detected ?? { languages: [], packageManager: null, projects: 0 }) };
      }),
    install: (answers) =>
      log('install', [answers], () => {
        if (seed.installError) throw new Error(seed.installError);
        if (!view.linuxFeature) throw new Error('SANDBOX_NEEDS_LINUX_FEATURE: Needs Windows\' Linux feature (admin + restart).');
        view.install = { ...emptyInstall(), state: 'running' };
        view.state = 'installing';
        emitInstall();
      }),
    continueInstall: () =>
      log('continueInstall', [], () => {
        if (view.state !== 'incomplete') throw new Error('SANDBOX_NOT_UNFINISHED: There is no unfinished set up to carry on.');
        if (!view.canContinue) throw new Error('SANDBOX_CANNOT_CONTINUE: This set up cannot be carried on. Start over instead.');
        view.install = { ...emptyInstall(), state: 'running' };
        view.state = 'installing';
        emitInstall();
      }),
    cancelInstall: () =>
      log('cancelInstall', [], () => {
        view.install = { ...view.install, cancelling: true };
        emitInstall();
      }),
    dismissInstall: () =>
      log('dismissInstall', [], () => {
        if (view.install.state !== 'running') view.install = emptyInstall();
      }),
    start: () =>
      log('start', [], () => {
        view.state = 'running';
        emitChanged();
      }),
    stop: () =>
      log('stop', [], () => {
        view.state = 'stopped';
        emitChanged();
      }),
    reset: () =>
      log('reset', [], () => {
        view.state = 'stopped';
        emitChanged();
      }),
    remove: (typed) =>
      log('remove', [typed], () => {
        if (typed.trim().toLowerCase() !== REMOVE_WORD) throw new Error('SANDBOX_CONFIRM_WORD: Type the word remove to remove the sandbox.');
        view.state = 'not_installed';
        view.reason = null;
        view.canContinue = false;
        view.hasResetPoint = false;
        view.diskBytes = null;
        view.install = emptyInstall();
        emitChanged();
      }),
    setIdle: (minutes) =>
      log('setIdle', [minutes], () => {
        view.idleMinutes = Math.min(240, Math.max(1, Math.round(minutes)));
        return view.idleMinutes;
      }),
    linuxAsk: () =>
      log('linuxAsk', [], () => ({
        token: 'fake-token',
        title: "Turn on Windows' Linux feature?",
        body: "Windows will ask for an administrator's OK, and you will most likely have to restart the computer afterwards.",
      })),
    linuxEnable: (token) =>
      log('linuxEnable', [token], () => {
        if (token !== 'fake-token') throw new Error('SANDBOX_NOT_CONFIRMED: Press the button again.');
        view.linuxFeature = true;
        return { restartNeeded: true };
      }),
    onInstall: (cb) => {
      installListeners.add(cb);
      return Promise.resolve((() => installListeners.delete(cb)) as UnlistenFn);
    },
    onChanged: (cb) => {
      changedListeners.add(cb);
      return Promise.resolve((() => changedListeners.delete(cb)) as UnlistenFn);
    },
  };
}
