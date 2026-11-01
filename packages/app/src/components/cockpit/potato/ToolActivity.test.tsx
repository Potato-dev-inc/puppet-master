// @vitest-environment jsdom
// The folded tool line after a failed step: "Couldn't <verb> · Try again" sends the retry words as the person; after two
// failures in a row it offers "Take control" instead. A successful or non-last failure changes nothing.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TRY_AGAIN_TEXT, toolStep, type ToolStep } from '../../../lib/potato-thread';
import { ToolActivity } from './ToolActivity';

const step = (tool: string, ok = true, id = `${tool}-${Math.random()}`): ToolStep =>
  toolStep({ id, text: `Fish used ${tool}: ${ok ? 'ok' : 'failed LEASE_LOST'}` })!;

let host: HTMLDivElement;
let root: Root;

const render = (ui: Parameters<Root['render']>[0]) => {
  act(() => root.render(ui));
};
const foldedLine = () => host.querySelector('.ck-potato-activity-title')?.textContent ?? '';
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text) ?? null;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('ToolActivity after a failed step', () => {
  it('reads "Couldn\'t open the page" with Try again, and Try again sends the exact retry text', () => {
    const onSendText = vi.fn();
    render(<ToolActivity steps={[step('browser_navigate', false)]} onSendText={onSendText} />);
    expect(foldedLine()).toBe("Couldn't open the page");
    act(() => button('Try again')!.click());
    expect(onSendText).toHaveBeenCalledTimes(1);
    expect(onSendText).toHaveBeenCalledWith('Please try that again.');
    expect(TRY_AGAIN_TEXT).toBe('Please try that again.');
  });

  it('offers Take control, not Try again, when the same step failed twice in a row', () => {
    const onSendText = vi.fn();
    const onTakeControl = vi.fn();
    render(<ToolActivity steps={[step('browser_navigate', false), step('browser_navigate', false)]} onSendText={onSendText} onTakeControl={onTakeControl} />);
    expect(button('Try again')).toBeNull();
    act(() => button('Take control')!.click());
    expect(onTakeControl).toHaveBeenCalledTimes(1);
    expect(onSendText).not.toHaveBeenCalled();
  });

  it('counts the previous attempt from the run before', () => {
    render(<ToolActivity steps={[step('browser_navigate', false)]} before={step('browser_navigate', false)} onTakeControl={() => {}} onSendText={() => {}} />);
    expect(button('Take control')).not.toBeNull();
  });

  it('never retries on its own', () => {
    vi.useFakeTimers();
    const onSendText = vi.fn();
    render(<ToolActivity steps={[step('browser_navigate', false)]} onSendText={onSendText} />);
    act(() => vi.runAllTimers());
    expect(onSendText).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  it('changes nothing when the run succeeded', () => {
    render(<ToolActivity steps={[step('browser_navigate'), step('browser_act')]} onSendText={() => {}} />);
    expect(foldedLine()).toBe('Used the computer · 2 steps');
    expect(button('Try again')).toBeNull();
  });

  it('changes nothing when only an earlier step failed', () => {
    render(<ToolActivity steps={[step('browser_navigate', false), step('browser_act')]} onSendText={() => {}} />);
    expect(foldedLine()).toBe('Used the computer · 2 steps · 1 failed');
    expect(button('Try again')).toBeNull();
  });
});
