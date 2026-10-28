import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  REGISTRY_LABEL,
  firstPaintRows,
  pageUrlFor,
  packageCheckLookup,
  packageCheckSettingsGet,
  packageCheckSettingsSet,
  type Ecosystem,
  type PackageItem,
  type PackageList,
} from './package-check';

const RUST_MODEL = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../src-tauri/src/prompt_risk/packages/model.rs',
);

// The same JSON the Rust golden test (`package_item_golden_json_camel_case`) pins.
const lodahs: PackageItem = {
  ecosystem: 'npm',
  tool: 'npm',
  name: 'lodahs',
  spec: '^1',
  source: { kind: 'registry' },
  global: false,
  runsNow: false,
  lookalike: { popular: 'lodash', how: 'swapped' },
  popular: false,
  cut: false,
  lookup: true,
  first: 'Looks like lodash (2 letters swapped) · Checking…',
  look: true,
};

const item = (patch: Partial<PackageItem>): PackageItem => ({ ...lodahs, ...patch });

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
});

describe('package check command arguments', () => {
  it('sends the pane and prompt ids in camelCase, and again=false by default', async () => {
    await packageCheckLookup('pane-a', 'prompt-1');
    expect(invoke).toHaveBeenCalledWith('package_check_lookup', {
      paneId: 'pane-a',
      promptId: 'prompt-1',
      again: false,
    });
  });

  it('sends again=true for Check again', async () => {
    await packageCheckLookup('pane-a', 'prompt-1', true);
    expect(invoke).toHaveBeenLastCalledWith('package_check_lookup', {
      paneId: 'pane-a',
      promptId: 'prompt-1',
      again: true,
    });
  });

  it('reads the setting without arguments', async () => {
    invoke.mockResolvedValueOnce({ lookups: true });
    await expect(packageCheckSettingsGet()).resolves.toEqual({ lookups: true });
    expect(invoke).toHaveBeenCalledWith('package_check_settings_get');
  });

  it('writes the setting as one settings argument', async () => {
    await packageCheckSettingsSet({ lookups: false });
    expect(invoke).toHaveBeenCalledWith('package_check_settings_set', { settings: { lookups: false } });
  });
});

describe('pageUrlFor (mirrors Rust page_url)', () => {
  it('builds the registry page per ecosystem', () => {
    expect(pageUrlFor('npm', 'lodash')).toBe('https://www.npmjs.com/package/lodash');
    expect(pageUrlFor('pypi', 'Foo_Bar')).toBe('https://pypi.org/project/foo-bar/');
    expect(pageUrlFor('crates', 'serde')).toBe('https://crates.io/crates/serde');
    expect(pageUrlFor('gems', 'rails')).toBe('https://rubygems.org/gems/rails');
    expect(pageUrlFor('go', 'golang.org/x/tools/gopls')).toBe('https://pkg.go.dev/golang.org/x/tools/gopls');
  });

  it('has no page for winget, Chocolatey or Scoop', () => {
    expect(pageUrlFor('winget', 'Git.Git')).toBeNull();
    expect(pageUrlFor('choco', 'nodejs')).toBeNull();
    expect(pageUrlFor('scoop', 'git')).toBeNull();
  });
});

describe('firstPaintRows', () => {
  it('gives one row per item, in order, and fills the first line in place', () => {
    const list: PackageList = {
      items: [lodahs, item({ name: 'left-pad', lookalike: undefined, look: false, first: 'left-pad · Checking…' })],
      more: 0,
      hidden: 0,
    };
    const rows = firstPaintRows(list);
    expect(rows.map((r) => r.name)).toEqual(['lodahs', 'left-pad']);
    expect(rows[0]).toEqual({
      name: 'lodahs',
      ecosystem: 'npm',
      registryLabel: 'npm',
      state: 'checking',
      look: true,
      summary: 'Looks like lodash (2 letters swapped) · Checking…',
      lines: [],
      pageUrl: 'https://www.npmjs.com/package/lodahs',
    });
    expect(rows[1].state).toBe('checking');
    expect(rows[1].look).toBe(false);
  });

  it('marks items that get no lookup as not_checked, with no link when they are not registry names', () => {
    const rows = firstPaintRows({
      items: [
        item({
          ecosystem: 'winget',
          tool: 'winget',
          name: 'Git.Git',
          source: { kind: 'registry' },
          lookup: false,
          lookalike: undefined,
          look: false,
          first: 'Not checked: Puppet Master can\'t look up winget packages yet',
        }),
        item({
          name: './local-dir',
          source: { kind: 'local' },
          lookup: false,
          first: 'Installs from a folder or file on this computer. Not checked.',
        }),
        item({ name: 'react-dom', source: { kind: 'registry' }, cut: true, lookup: false, first: 'Not checked.' }),
      ],
      more: 0,
      hidden: 0,
    });
    expect(rows.map((r) => r.state)).toEqual(['not_checked', 'not_checked', 'not_checked']);
    expect(rows[0].registryLabel).toBe('winget');
    expect(rows[0].pageUrl).toBeNull();
    expect(rows[1].pageUrl).toBeNull();
    expect(rows[2].pageUrl).toBeNull();
    expect(rows.every((r) => r.lines.length === 0)).toBe(true);
  });

  it('returns no rows for an empty list', () => {
    expect(firstPaintRows({ items: [], more: 0, hidden: 0 })).toEqual([]);
  });
});

describe('REGISTRY_LABEL mirrors the Rust registry_label', () => {
  const rust = readFileSync(RUST_MODEL, 'utf8');
  const variant: Record<Ecosystem, string> = {
    npm: 'Npm',
    pypi: 'Pypi',
    crates: 'Crates',
    go: 'Go',
    gems: 'Gems',
    winget: 'Winget',
    choco: 'Choco',
    scoop: 'Scoop',
  };

  it.each(Object.keys(variant) as Ecosystem[])('%s label is the same text as Rust', (eco) => {
    const label = REGISTRY_LABEL[eco];
    const escaped = label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    expect(rust).toMatch(new RegExp(`Self::${variant[eco]} => "${escaped}"`));
  });
});
