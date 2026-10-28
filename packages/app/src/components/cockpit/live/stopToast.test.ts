// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { stopSentence } from './stopToast';
import { clockText } from './GoLiveSheet';

const ENDS = new Date(2030, 0, 1, 15, 40).getTime();

describe('stopSentence', () => {
  it('says each reason in plain English', () => {
    expect(stopSentence('owner', ENDS)).toBe('Live Room stopped.');
    expect(stopSentence('expired', ENDS)).toBe('Live Room ended: time is up.');
    expect(stopSentence('idle', ENDS)).toBe('Live Room stopped: nothing happened for 20 minutes.');
    expect(stopSentence('all_panes_closed', ENDS)).toBe('Live Room stopped: its panes are closed.');
    expect(stopSentence('relay_ended', ENDS)).toBe('Live Room ended by the relay.');
    expect(stopSentence('clock_jump', ENDS)).toBe('Live Room stopped while this PC was asleep.');
  });

  it('tells when the link ends by itself after the relay was lost', () => {
    expect(stopSentence('relay_lost', ENDS)).toBe(`Live Room stopped here. The link ends by itself at ${clockText(ENDS)}.`);
    expect(clockText(ENDS)).toBe('15:40');
    expect(stopSentence('relay_lost', 0)).toBe('Live Room stopped here. The link ends by itself.');
  });

  it('says nothing when the app is closing, and falls back to the plain sentence for a word it does not know', () => {
    expect(stopSentence('app_quit', ENDS)).toBeNull();
    expect(stopSentence('something_new', ENDS)).toBe('Live Room stopped.');
    expect(stopSentence(null, ENDS)).toBe('Live Room stopped.');
  });

  it('never carries a link', () => {
    for (const r of ['owner', 'expired', 'idle', 'all_panes_closed', 'relay_lost', 'relay_ended', 'clock_jump']) {
      expect(stopSentence(r, ENDS)).not.toMatch(/https?:|#/);
    }
  });
});
