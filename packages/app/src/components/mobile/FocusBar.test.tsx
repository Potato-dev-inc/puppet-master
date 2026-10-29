// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from '../../lib/bridge-types';
import type { PhoneFocus } from '../../lib/focus';
import { FocusBar, resetFocusBarForTests } from './FocusBar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MIN = 60_000;
const REFUSED = "Couldn't change Focus. Try again.";

function off(): PhoneFocus {
  return { on: false, endsAtMs: null, minutes: 0, heldTotal: 0, waiting: 0, last: null };
}

function on(minutesLeft = 31): PhoneFocus {
  return { on: true, endsAtMs: Date.now() + minutesLeft * MIN + 30_000, minutes: 45, heldTotal: 3, waiting: 0, last: null };
}

const ENDED: PhoneFocus = {
  on: false,
  endsAtMs: null,
  minutes: 45,
  heldTotal: 0,
  waiting: 0,
  last: { ended_at_ms: 1_800_000_000_000, line: 'While you focused (45 min): 4 turns done, 1 failed.', ended_by: 'timer' },
};

function fakeBridge(first: PhoneFocus | Error = off()) {
  const focus = vi.fn(async () => {
    if (first instanceof Error) throw first;
    return first;
  });
  const focusStart = vi.fn(async (_minutes: number) => on());
  const focusEnd = vi.fn(async () => ENDED);
  const bridge = { focus, focusStart, focusEnd } as unknown as BridgeClient;
  return { bridge, focus, focusStart, focusEnd };
}

let host: HTMLDivElement;
let root: Root;

async function mount(bridge: BridgeClient, props: { canControl?: boolean; bridgeReady?: boolean; focusRevision?: number } = {}) {
  await act(async () => {
    root.render(
      <FocusBar
        bridge={bridge}
        bridgeReady={props.bridgeReady ?? true}
        canControl={props.canControl ?? true}
        focusRevision={props.focusRevision}
      />,
    );
  });
}

async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}

const buttons = () => Array.from(host.querySelectorAll<HTMLButtonElement>('button'));
const buttonText = (text: string) => buttons().find((b) => b.textContent?.trim() === text);
const card = () => host.querySelector<HTMLElement>('[aria-label="Focus"]');
const headline = () => card()?.querySelector('.ph-card-q')?.textContent;

beforeEach(() => {
  resetFocusBarForTests();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('FocusBar', () => {
  it('draws nothing when the bridge has no focus route (an old PC)', async () => {
    const bridge = {} as BridgeClient;
    await mount(bridge);
    expect(card()).toBeNull();
    expect(host.textContent).toBe('');
  });

  it('shows the off state with the three length chips when the phone has control', async () => {
    const { bridge, focus } = fakeBridge(off());
    await mount(bridge);
    expect(focus).toHaveBeenCalledTimes(1);
    expect(headline()).toBe('Focus');
    expect(buttons().map((b) => b.textContent?.trim())).toEqual(['25', '45', '90']);
  });

  it('starts a Focus with the chosen length and shows it on', async () => {
    const { bridge, focusStart } = fakeBridge(off());
    await mount(bridge);
    await click(buttonText('45'));
    expect(focusStart).toHaveBeenCalledWith(45);
    expect(headline()).toBe('Focus · 31 min left');
  });

  it('shows the on state with the time left and an End button', async () => {
    const { bridge } = fakeBridge(on(31));
    await mount(bridge);
    expect(headline()).toBe('Focus · 31 min left');
    expect(buttonText('End')).toBeTruthy();
    expect(buttonText('25')).toBeFalsy();
  });

  it('End calls focusEnd and then shows the summary line with Got it', async () => {
    const { bridge, focusEnd } = fakeBridge(on(31));
    await mount(bridge);
    await click(buttonText('End'));
    expect(focusEnd).toHaveBeenCalledTimes(1);
    expect(headline()).toBe(ENDED.last?.line);
    expect(buttonText('Got it')).toBeTruthy();
    expect(buttonText('End')).toBeFalsy();
  });

  it('shows the summary after an end the PC reports on a read', async () => {
    const { bridge } = fakeBridge(ENDED);
    await mount(bridge);
    expect(headline()).toBe(ENDED.last?.line);
  });

  it('Got it dismisses the summary on this device only, without calling the PC', async () => {
    const { bridge, focus, focusEnd } = fakeBridge(ENDED);
    await mount(bridge);
    await click(buttonText('Got it'));
    expect(focusEnd).not.toHaveBeenCalled();
    expect(focus).toHaveBeenCalledTimes(1);
    expect(headline()).toBe('Focus');
    expect(buttonText('Got it')).toBeFalsy();
    expect(buttonText('25')).toBeTruthy();
  });

  it('shows the could-not-change line when the PC refuses a start with 403', async () => {
    const { bridge, focusStart } = fakeBridge(off());
    focusStart.mockRejectedValueOnce(new Error('bridge POST /focus/start -> 403: PERSON_ONLY'));
    await mount(bridge);
    await click(buttonText('25'));
    expect(host.querySelector('.ph-appr-status')?.textContent).toBe(REFUSED);
    expect(headline()).toBe('Focus');
  });

  it('a refused End leaves the bar on and shows the same line', async () => {
    const { bridge, focusEnd } = fakeBridge(on(20));
    focusEnd.mockRejectedValueOnce(new Error('bridge POST /focus/end -> 403: PERSON_ONLY'));
    await mount(bridge);
    await click(buttonText('End'));
    expect(host.querySelector('.ph-appr-status')?.textContent).toBe(REFUSED);
    expect(headline()).toBe('Focus · 20 min left');
  });

  it('a bumped focusRevision (the focus event) reads the PC again and changes the bar', async () => {
    const { bridge, focus } = fakeBridge(off());
    await mount(bridge, { focusRevision: 0 });
    expect(headline()).toBe('Focus');
    focus.mockResolvedValueOnce(on(44));
    await mount(bridge, { focusRevision: 1 });
    expect(focus).toHaveBeenCalledTimes(2);
    expect(headline()).toBe('Focus · 44 min left');
  });

  it('without the control cap it only shows the state: no chips and no End', async () => {
    const offBar = fakeBridge(off());
    await mount(offBar.bridge, { canControl: false });
    expect(buttons()).toHaveLength(0);
    expect(card()?.textContent).toContain('Off');
    await act(async () => root.unmount());
    root = createRoot(host);

    const onBar = fakeBridge(on(12));
    await mount(onBar.bridge, { canControl: false });
    expect(headline()).toBe('Focus · 12 min left');
    expect(buttons()).toHaveLength(0);
  });

  it('reads nothing until the bridge is ready', async () => {
    const { bridge, focus } = fakeBridge(off());
    await mount(bridge, { bridgeReady: false });
    expect(focus).not.toHaveBeenCalled();
    expect(card()).toBeNull();
  });
});
