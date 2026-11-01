import { describe, expect, it } from 'vitest';
import { addonFor, blockOf, needsInstall, parseList, parseStatus, plainMessage, progressText, promptText, rowText, sizeText, type AddonStatus } from './acpAddon';

const status = (over: Partial<AddonStatus> = {}): AddonStatus => ({
  agent: 'claude',
  label: 'Claude',
  state: 'not_installed',
  version: null,
  pinned_version: '0.74.0',
  detail: '',
  size_mb: 48,
  lines: [],
  signin_hint: 'Sign in in the terminal.',
  ...over,
});

describe('the words', () => {
  it('the install prompt names the agent and about how big the add-on is', () => {
    expect(promptText(status())).toBe('Chat needs a small add-on for Claude (about 48 MB).');
    expect(promptText(status({ size_mb: null }))).toBe('Chat needs a small add-on for Claude.');
    expect(sizeText(0)).toBe('');
  });

  it('the label is the one the backend gives (Claude, never Claude Code)', () => {
    expect(promptText(status())).not.toContain('Claude Code');
  });

  it('progress is the newest line, or a plain Installing…', () => {
    expect(progressText(status({ state: 'installing' }))).toBe('Installing…');
    expect(progressText(status({ state: 'installing', lines: ['a', 'b'] }))).toBe('b');
  });

  it('the Settings row says installed with the version, not installed, installing or why it failed', () => {
    expect(rowText(status({ state: 'installed', version: '0.74.0' }))).toBe('Installed 0.74.0');
    expect(rowText(status())).toBe('Not installed');
    expect(rowText(status({ state: 'installing', lines: ['Downloading…'] }))).toBe('Installing… Downloading…');
    expect(rowText(status({ state: 'failed', detail: 'Node.js is needed.' }))).toBe('Node.js is needed.');
  });

  it('chat needs the install unless it is installed', () => {
    expect(needsInstall(undefined)).toBe(false);
    expect(needsInstall(status())).toBe(true);
    expect(needsInstall(status({ state: 'installing' }))).toBe(true);
    expect(needsInstall(status({ state: 'failed' }))).toBe(true);
    expect(needsInstall(status({ state: 'installed' }))).toBe(false);
  });
});

describe('what comes from the backend', () => {
  it('reads a list and drops anything odd', () => {
    const list = parseList({
      labs_enabled: true,
      addons: [{ agent: 'codex', label: 'Codex', state: 'installed', version: '1.1.4', size_mb: 16, lines: ['x', 3], signin_hint: 'h', detail: '' }, { agent: 5 }, null, { agent: 'x', state: 'weird' }],
    });
    expect(list.labsEnabled).toBe(true);
    expect(list.addons).toHaveLength(1);
    expect(list.addons[0]).toMatchObject({ agent: 'codex', version: '1.1.4', lines: ['x'], size_mb: 16 });
    expect(parseList(null)).toEqual({ labsEnabled: false, addons: [] });
    expect(parseList({ labs_enabled: 'yes' as unknown as boolean }).labsEnabled).toBe(false);
  });

  it('a status without a label falls back to the id', () => {
    expect(parseStatus({ agent: 'zed', state: 'failed' })?.label).toBe('zed');
  });

  it('finds an add-on without regard to case', () => {
    const by = { claude: status() };
    expect(addonFor(by, 'Claude')?.agent).toBe('claude');
    expect(addonFor(by, 'other')).toBeUndefined();
    expect(addonFor(by, null)).toBeUndefined();
  });
});

describe('telling a start error apart', () => {
  it('the two codes a card answers', () => {
    expect(blockOf('ACP_ADDON_MISSING: Chat needs a small add-on for Claude (about 48 MB). Install it first.')).toBe('addon');
    expect(blockOf('ACP_SIGN_IN_NEEDED: The agent needs you to sign in first. The terminal can run this agent instead.')).toBe('signin');
    expect(blockOf('ACP_START_FAILED: The agent could not start')).toBeNull();
    expect(blockOf(undefined)).toBeNull();
  });

  it('the sentence without its code', () => {
    expect(plainMessage('ACP_ADDON_BUSY: The add-on is already being installed.')).toBe('The add-on is already being installed.');
    expect(plainMessage('Just a sentence.')).toBe('Just a sentence.');
  });
});
