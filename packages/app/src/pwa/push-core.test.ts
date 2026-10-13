import { describe, expect, it } from 'vitest';
import { clickIntent, isOpenPaneMessage, openUrl, paneFromSearch, parsePushPayload, planNotification } from './push-core';

describe('parsePushPayload', () => {
  it('reads the server payload', () => {
    const p = parsePushPayload(
      JSON.stringify({ v: 1, kind: 'waiting_input', title: 'Kilo needs you', body: 'Run command: ls', tag: 'pm-p1', pane_id: 'p1', prompt_id: 'x', act: { url: '/bridge/push/act', token: 'T' } }),
    );
    expect(p).toMatchObject({ title: 'Kilo needs you', pane_id: 'p1', act: { url: '/bridge/push/act', token: 'T' } });
  });

  it('never throws and falls back to a generic notification (iOS needs one for every push)', () => {
    for (const bad of [undefined, null, '', 'not json', '{"title":5}']) {
      const p = parsePushPayload(bad as string | null | undefined);
      expect(p.title.length).toBeGreaterThan(0);
      expect(p.body.length).toBeGreaterThan(0);
    }
  });

  it('refuses an act url that leaves the origin', () => {
    expect(parsePushPayload(JSON.stringify({ act: { url: 'https://evil.example/x', token: 'T' } })).act).toBeUndefined();
  });
});

describe('planNotification', () => {
  it('adds Allow / Deny only when a token is present, and renotifies per prompt', () => {
    const plain = planNotification(parsePushPayload('{"title":"Codex finished","body":"The run is done","tag":"pm-1"}'));
    expect(plain.options.actions).toBeUndefined();
    expect(plain.options.renotify).toBe(true);
    const actionable = planNotification(parsePushPayload('{"title":"t","body":"b","tag":"x","pane_id":"p","act":{"url":"/bridge/push/act","token":"T"}}'));
    expect(actionable.options.actions?.map((a) => a.action)).toEqual(['allow', 'deny']);
  });
});

describe('clickIntent', () => {
  const data = { pane_id: 'p9', act: { url: '/bridge/push/act', token: 'T' } };
  it('answers on the buttons, opens the pane on a plain tap', () => {
    expect(clickIntent('allow', data)).toEqual({ type: 'answer', url: '/bridge/push/act', token: 'T', choice: 'allow' });
    expect(clickIntent('deny', data)).toMatchObject({ type: 'answer', choice: 'deny' });
    expect(clickIntent('', data)).toEqual({ type: 'open', paneId: 'p9' });
  });

  it('opens instead of answering when there is no token', () => {
    expect(clickIntent('allow', { pane_id: 'p9' })).toEqual({ type: 'open', paneId: 'p9' });
    expect(clickIntent('', undefined)).toEqual({ type: 'open', paneId: null });
  });
});

describe('deep links', () => {
  it('round-trips the pane id through the launch url and the focus message', () => {
    expect(openUrl('a b')).toBe('/?pane=a%20b');
    expect(openUrl(null)).toBe('/');
    expect(paneFromSearch('?pane=a%20b')).toBe('a b');
    expect(paneFromSearch('?x=1')).toBeNull();
    expect(isOpenPaneMessage({ type: 'pm-open-pane', pane_id: 'p' })).toBe(true);
    expect(isOpenPaneMessage({ type: 'other' })).toBe(false);
    expect(isOpenPaneMessage(null)).toBe(false);
  });
});
