// Package check for the harness: the packages a mocked approval installs and what the mocked
// `package_check_*` commands answer. Made-up package names only. Shapes: `lib/package-check.ts`.
import type { PanePrompt } from '../lib/cockpit-api';
import type { FactLine, PackageCheckView, PackageItem, PackageList, PackageRow } from '../lib/package-check';
import { EXAMPLE_PROMPT, PANE_CLAUDE } from './fixtures';

export interface PackageCheckScenario {
  /** The packages the approval prompt carries (`pane_prompt` for the example Claude pane). */
  list: PackageList;
  /** The prompt the packages ride on (default: the example Bash approval). */
  prompt?: PanePrompt;
  /** What `package_check_lookup` answers: a view, `error` (the command fails) or `hang` (never answers). */
  lookup: PackageCheckView | 'error' | 'hang';
  /** Settings > General > Safety > Check packages before installs (default on). */
  lookups?: boolean;
}

const NPM_FOOTER = 'Facts from the public npm registry. Only the package names were sent. Puppet Master never answers for you.';

function item(over: Pick<PackageItem, 'ecosystem' | 'tool' | 'name' | 'first'> & Partial<PackageItem>): PackageItem {
  return { source: { kind: 'registry' }, global: false, runsNow: false, popular: false, cut: false, lookup: false, look: false, ...over };
}

function row(over: Pick<PackageRow, 'name' | 'ecosystem' | 'registryLabel' | 'state' | 'summary'> & Partial<PackageRow>): PackageRow {
  return { look: false, lines: [], pageUrl: null, ...over };
}

const fact = (text: string, look = false): FactLine => ({ look, text });

/** `npm install lodahs`: one look-alike name. The offline line is there at once; the facts follow in the same row. */
export const LODAHS_ITEM = item({
  ecosystem: 'npm',
  tool: 'npm',
  name: 'lodahs',
  lookalike: { popular: 'lodash', how: 'swapped' },
  lookup: true,
  look: true,
  first: 'Looks like lodash (2 letters swapped) · Checking…',
});

export const LOOKALIKE_LIST: PackageList = { items: [LODAHS_ITEM], more: 0, hidden: 0 };

export const LOOKALIKE_VIEW: PackageCheckView = {
  promptId: 'prompt-1',
  off: false,
  statusText: 'Checked just now',
  look: 1,
  rows: [
    row({
      name: 'lodahs',
      ecosystem: 'npm',
      registryLabel: 'npm',
      state: 'checked',
      look: true,
      summary: 'Looks like lodash (2 letters swapped) · New: first published 3 days ago · Few downloads: 12 a week',
      lines: [
        fact('Looks like lodash (2 letters swapped)', true),
        fact('New: first published 3 days ago', true),
        fact('Few downloads: 12 a week', true),
        fact('Runs a script when installed', true),
      ],
      pageUrl: 'https://www.npmjs.com/package/lodahs',
    }),
  ],
  more: 0,
  hidden: 0,
  footer: NPM_FOOTER,
  canRetry: false,
};

/** `npm i mapforge-kit`: an ordinary package. Plain facts only, no "Look closer". */
export const MAPFORGE_ITEM = item({
  ecosystem: 'npm',
  tool: 'npm',
  name: 'mapforge-kit',
  lookup: true,
  first: 'Checking…',
});

export const PLAIN_LIST: PackageList = { items: [MAPFORGE_ITEM], more: 0, hidden: 0 };

export const PLAIN_VIEW: PackageCheckView = {
  promptId: 'prompt-1',
  off: false,
  statusText: 'Checked just now',
  look: 0,
  rows: [
    row({
      name: 'mapforge-kit',
      ecosystem: 'npm',
      registryLabel: 'npm',
      state: 'checked',
      summary: 'First published 4 years ago · 52K downloads a week · 2 maintainers',
      lines: [fact('First published 4 years ago'), fact('52K downloads a week'), fact('2 maintainers')],
      pageUrl: 'https://www.npmjs.com/package/mapforge-kit',
    }),
  ],
  more: 0,
  hidden: 0,
  footer: NPM_FOOTER,
  canRetry: false,
};

/** Settings turned off: the look-alike still shows, every lookup row says so. */
export const OFF_VIEW: PackageCheckView = {
  promptId: 'prompt-1',
  off: true,
  statusText: 'Lookups are off',
  look: 1,
  rows: [
    row({
      name: 'lodahs',
      ecosystem: 'npm',
      registryLabel: 'npm',
      state: 'not_checked',
      look: true,
      summary: 'Looks like lodash (2 letters swapped) · Not checked: package lookups are off',
      lines: [fact('Looks like lodash (2 letters swapped)', true), fact('Not checked: package lookups are off')],
    }),
  ],
  more: 0,
  hidden: 0,
  footer: 'Look-alike names are still checked on this computer. Turn lookups on in Settings › General › Safety.',
  canRetry: false,
};

/** Seven packages from five ecosystems, with every row state, a secret-looking name and a cut-off list. */
export const MANY_LIST: PackageList = {
  items: [
    item({ ecosystem: 'npm', tool: 'npm', name: 'fluxtrail', lookup: true, first: 'Checking…' }),
    item({ ecosystem: 'pypi', tool: 'pip', name: 'quillbox', lookup: true, first: 'Checking…' }),
    item({ ecosystem: 'crates', tool: 'cargo', name: 'tidewarp-rs', lookup: true, first: 'Checking…' }),
    item({ ecosystem: 'npm', tool: 'npm', name: 'gridmint', lookup: true, first: 'Checking…' }),
    item({ ecosystem: 'gems', tool: 'gem', name: 'pinecase', lookup: true, first: 'Checking…' }),
    item({ ecosystem: 'winget', tool: 'winget', name: 'Wintool.Extra', first: "Not checked: Puppet Master can't look up winget packages yet" }),
    item({ ecosystem: 'npm', tool: 'npm', name: 'local-pkg', source: { kind: 'local' }, first: 'Installs from a folder or file on this computer. Not checked.' }),
  ],
  more: 2,
  hidden: 1,
};

export const MANY_VIEW: PackageCheckView = {
  promptId: 'prompt-1',
  off: false,
  statusText: 'Partly checked',
  look: 1,
  rows: [
    row({ name: 'fluxtrail', ecosystem: 'npm', registryLabel: 'npm', state: 'checked', summary: 'First published 2 years ago · 9K downloads a week', lines: [fact('First published 2 years ago'), fact('9K downloads a week')], pageUrl: 'https://www.npmjs.com/package/fluxtrail' }),
    row({ name: 'quillbox', ecosystem: 'pypi', registryLabel: 'PyPI', state: 'not_found', summary: 'Not found on PyPI. The name may be made up.', lines: [fact('Not found on PyPI. The name may be made up.', true)], pageUrl: null }),
    row({ name: 'tidewarp-rs', ecosystem: 'crates', registryLabel: 'crates.io', state: 'failed', summary: 'Could not check: crates.io did not answer in time', lines: [fact('Could not check: crates.io did not answer in time')], pageUrl: 'https://crates.io/crates/tidewarp-rs' }),
    row({ name: 'gridmint', ecosystem: 'npm', registryLabel: 'npm', state: 'checked', look: true, summary: 'Runs a script when installed · First published 5 days ago', lines: [fact('Runs a script when installed', true), fact('First published 5 days ago', true)], pageUrl: 'https://www.npmjs.com/package/gridmint' }),
    row({ name: 'pinecase', ecosystem: 'gems', registryLabel: 'RubyGems', state: 'checked', summary: 'Few downloads: 800 in total', lines: [fact('Few downloads: 800 in total', true)], pageUrl: 'https://rubygems.org/gems/pinecase' }),
    row({ name: 'Wintool.Extra', ecosystem: 'winget', registryLabel: 'winget', state: 'not_checked', summary: "Not checked: Puppet Master can't look up winget packages yet", lines: [fact("Not checked: Puppet Master can't look up winget packages yet")] }),
    row({ name: 'local-pkg', ecosystem: 'npm', registryLabel: 'npm', state: 'not_checked', summary: 'Installs from a folder or file on this computer. Not checked.', lines: [fact('Installs from a folder or file on this computer. Not checked.')] }),
  ],
  more: 2,
  hidden: 1,
  footer: NPM_FOOTER,
  canRetry: true,
};

/** A prompt that carries the look-alike install as a High risk approval (Deny first). */
export const LOOKALIKE_PROMPT: PanePrompt = {
  ...EXAMPLE_PROMPT,
  promptId: 'prompt-1',
  packages: LOOKALIKE_LIST,
  kind: 'permission',
  text: 'Do you want to run this command?\n  npm install lodahs',
  summary: 'Run npm install lodahs',
  risk: {
    level: 'high',
    headline: 'Install a package with a look-alike name',
    reasons: ['lodahs looks like the popular package lodash (2 letters swapped). Check the name before you allow it.'],
  },
};

export const PLAIN_PROMPT: PanePrompt = {
  ...EXAMPLE_PROMPT,
  promptId: 'prompt-1',
  packages: PLAIN_LIST,
  text: 'Do you want to run this command?\n  npm install mapforge-kit',
  summary: 'Run npm install mapforge-kit',
  risk: { level: 'medium', headline: 'Install packages for this project', reasons: ['It adds packages to this project.'] },
};

export const MANY_PROMPT: PanePrompt = {
  ...PLAIN_PROMPT,
  packages: MANY_LIST,
  text: 'Do you want to run this command?\n  npm install fluxtrail gridmint local-pkg',
  summary: 'Run a command that installs seven packages',
};

const EMPTY_VIEW: PackageCheckView = { promptId: '', off: false, statusText: '', look: 0, rows: [], more: 0, hidden: 0, footer: '', canRetry: false };

/**
 * What the mocked package check answers. `handled: false` leaves the command to the normal mock. The
 * example Claude pane's approval carries the scenario's packages; `package_check_lookup` answers with the
 * scenario's view for whichever prompt it is asked about.
 */
export function packageCheckMock(s: PackageCheckScenario | undefined, cmd: string, args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (cmd === 'pane_prompt' && s && args.paneId === PANE_CLAUDE) {
    return { handled: true, value: { ...(s.prompt ?? EXAMPLE_PROMPT), packages: s.list } };
  }
  switch (cmd) {
    case 'package_check_lookup': {
      const answer = s?.lookup ?? { ...EMPTY_VIEW, promptId: String(args.promptId) };
      if (answer === 'error') return { handled: true, value: Promise.reject(new Error('unreachable')) };
      if (answer === 'hang') return { handled: true, value: new Promise(() => undefined) };
      return { handled: true, value: { ...answer, promptId: String(args.promptId) } };
    }
    case 'package_check_settings_get':
      return { handled: true, value: { lookups: s?.lookups ?? true } };
    case 'package_check_settings_set':
      return { handled: true, value: args.settings };
    default:
      return { handled: false };
  }
}
