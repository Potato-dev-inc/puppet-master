// @vitest-environment node
// The words about the link between my computers. Fixtures live in this test file only (no sample data ships).
import { describe, expect, it } from 'vitest';
import { linkStateText, linkStatusText, type LinkDevice, type LinkView } from './desk-link';

function dev(over: Partial<LinkDevice> = {}): LinkDevice {
  return { node: 'ab'.repeat(32), name: 'Laptop', state: 'online', side: 'right', mouse_here: false, controls_me: false, input_on: true, clipboard_on: false, ...over };
}

function view(over: Partial<LinkView> = {}): LinkView {
  return {
    seq: 1,
    devices: [dev()],
    mouse_on: null,
    controlled_by: null,
    paused: false,
    sharing: true,
    tray: 'Mouse sharing is on',
    admin_note: "Can't control windows that run as admin.",
    error: null,
    clipboard_note: null,
    supported: true,
    ...over,
  };
}

describe('linkStateText', () => {
  it('says online, connecting and offline in plain words, and nothing while the switch is off', () => {
    expect(linkStateText('online')).toBe('Connected');
    expect(linkStateText('connecting')).toBe('Connecting…');
    expect(linkStateText('offline')).toBe('Offline');
    expect(linkStateText('off')).toBeNull();
    expect(linkStateText(undefined)).toBeNull();
  });
});

describe('linkStatusText', () => {
  it('says nothing before the link answers or on a computer it does not work on', () => {
    expect(linkStatusText(null)).toBeNull();
    expect(linkStatusText(view({ supported: false }))).toBeNull();
  });

  it('says nothing while no switch is on', () => {
    expect(linkStatusText(view({ sharing: false, tray: null, devices: [dev({ state: 'off', input_on: false })] }))).toBeNull();
  });

  it('says where the mouse is', () => {
    expect(linkStatusText(view({ mouse_on: 'Laptop' }))).toBe('Mouse is on Laptop');
    expect(linkStatusText(view({ controlled_by: 'Ren-PC' }))).toBe('Ren-PC is using this computer');
  });

  it('says it is on, with who is connected, or paused', () => {
    expect(linkStatusText(view())).toBe('Mouse sharing is on · Laptop connected');
    expect(linkStatusText(view({ devices: [dev({ state: 'offline' })] }))).toBe('Mouse sharing is on');
    expect(linkStatusText(view({ paused: true }))).toBe('Mouse sharing is paused');
  });

  it('puts an error first', () => {
    expect(linkStatusText(view({ error: 'Could not take the mouse: nope', mouse_on: 'Laptop' }))).toBe('Mouse sharing hit a problem: Could not take the mouse: nope');
  });
});
