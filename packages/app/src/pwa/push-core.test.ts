import { describe, expect, it } from 'vitest';
import {
  clickIntent,
  isOpenItemMessage,
  isOpenPaneMessage,
  itemFromSearch,
  isOpenViewMessage,
  openUrl,
  paneFromSearch,
  parsePushPayload,
  planNotification,
  viewFromSearch,
} from './push-core';

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

  it('keeps target "questions" and drops any other value', () => {
    expect(parsePushPayload('{"target":"questions"}').target).toBe('questions');
    expect(parsePushPayload('{"target":"panes"}').target).toBeUndefined();
    expect(parsePushPayload('{"target":5}').target).toBeUndefined();
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

  it('plans a question notification without buttons and carries the target', () => {
    const question = planNotification(
      parsePushPayload('{"v":1,"kind":"question","title":"Kilo asks","body":"Deploy?","tag":"pm-q-xyz","target":"questions"}'),
    );
    expect(question.options.actions).toBeUndefined();
    expect(question.options.data.target).toBe('questions');
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

  it('opens the question list for a question target', () => {
    expect(clickIntent('', { target: 'questions' })).toEqual({ type: 'open', paneId: null, view: 'questions' });
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

  it('opens the question list from a launch url or a focus message', () => {
    expect(openUrl(null, 'questions')).toBe('/?view=questions');
    expect(viewFromSearch('?view=questions')).toBe('questions');
    expect(viewFromSearch('?view=x')).toBeNull();
    expect(viewFromSearch('?pane=p')).toBeNull();
    expect(isOpenViewMessage({ type: 'pm-open-view', view: 'questions' })).toBe(true);
    expect(isOpenViewMessage({ type: 'pm-open-view', view: 'x' })).toBe(false);
    expect(isOpenViewMessage({ type: 'pm-open-pane', pane_id: 'p' })).toBe(false);
    expect(isOpenViewMessage(null)).toBe(false);
  });
});

describe('payload v2 and the card deep link', () => {
  const v2 = JSON.stringify({ v: 2, kind: 'waiting_input', title: 'cart rounding · Needs you', body: 'Low risk · Run tests\nCodex', tag: 'pm-p1', pane_id: 'p1', item_id: 'a7Kp2xQ9', prompt_id: 'x' });

  it('reads the item id of a v2 payload and carries it to the notification', () => {
    const payload = parsePushPayload(v2);
    expect(payload.item_id).toBe('a7Kp2xQ9');
    expect(planNotification(payload).options.data).toMatchObject({ item_id: 'a7Kp2xQ9', pane_id: 'p1' });
    expect(parsePushPayload('{"item_id":5}').item_id).toBeUndefined();
  });

  it('a tap opens the card first, then the pane, then the list', () => {
    expect(clickIntent('', { pane_id: 'p1', item_id: 'a7Kp2xQ9' })).toEqual({ type: 'open', paneId: 'p1', itemId: 'a7Kp2xQ9' });
    expect(openUrl({ itemId: 'a7Kp2xQ9', paneId: 'p1' })).toBe('/?item=a7Kp2xQ9');
    expect(openUrl({ paneId: 'p1' })).toBe('/?pane=p1');
    expect(openUrl({ view: 'questions' })).toBe('/?view=questions');
    expect(openUrl({})).toBe('/');
  });

  it('a version 1 payload (no item id) still opens the pane', () => {
    const v1 = parsePushPayload('{"v":1,"kind":"waiting_input","title":"t","body":"b","tag":"x","pane_id":"p9"}');
    expect(v1.item_id).toBeUndefined();
    expect(clickIntent('', planNotification(v1).options.data)).toMatchObject({ type: 'open', paneId: 'p9' });
    expect(openUrl({ itemId: undefined, paneId: 'p9' })).toBe('/?pane=p9');
  });

  it('the buttons still answer when the payload has an item', () => {
    const data = planNotification(parsePushPayload(JSON.stringify({ title: 't', body: 'b', tag: 'x', item_id: 'a7Kp2xQ9', act: { url: '/bridge/push/act', token: 'T' } }))).options.data;
    expect(clickIntent('allow', data)).toMatchObject({ type: 'answer', choice: 'allow' });
  });

  it('reads the card from a launch url and from the focus message, and refuses odd ids', () => {
    expect(itemFromSearch('?item=a7Kp2xQ9')).toBe('a7Kp2xQ9');
    expect(itemFromSearch('?pane=p')).toBeNull();
    expect(itemFromSearch('?item=a%20b')).toBeNull();
    expect(itemFromSearch(`?item=${'x'.repeat(41)}`)).toBeNull();
    expect(isOpenItemMessage({ type: 'pm-open-item', item_id: 'a7Kp2xQ9', pane_id: 'p' })).toBe(true);
    expect(isOpenItemMessage({ type: 'pm-open-item', item_id: '' })).toBe(false);
    expect(isOpenItemMessage({ type: 'pm-open-pane', pane_id: 'p' })).toBe(false);
    expect(isOpenItemMessage(null)).toBe(false);
  });
});

describe('quiet payload (Focus Time)', () => {
  it('a quiet payload is silent and does not renotify', () => {
    const plan = planNotification(parsePushPayload('{"v":1,"kind":"waiting_input","title":"Kilo needs you","body":"Run command: ls","tag":"pm-p1","pane_id":"p1","quiet":true}'));
    expect(plan.options.silent).toBe(true);
    expect(plan.options.renotify).toBe(false);
  });

  it('no quiet flag gives exactly today\'s options', () => {
    const plan = planNotification(parsePushPayload('{"title":"Codex finished","body":"The run is done","tag":"pm-1","pane_id":"p1"}'));
    expect(plan.options).toEqual({
      body: 'The run is done',
      tag: 'pm-1',
      renotify: true,
      silent: false,
      icon: '/icon-192.png',
      badge: '/icon-192.png',
      data: { pane_id: 'p1', item_id: undefined, act: undefined, target: undefined },
      actions: undefined,
    });
  });

  it('reads quiet only when it is exactly true; any other value is ignored', () => {
    for (const value of ['"yes"', '1', '"true"', 'false', 'null']) {
      const payload = parsePushPayload(`{"title":"t","body":"b","tag":"x","quiet":${value}}`);
      expect(payload.quiet).toBeUndefined();
      const plan = planNotification(payload);
      expect(plan.options.silent).toBe(false);
      expect(plan.options.renotify).toBe(true);
    }
    expect(parsePushPayload('{"quiet":true}').quiet).toBe(true);
  });

  it('a focus payload with no pane opens the app', () => {
    const payload = parsePushPayload('{"v":1,"kind":"focus","title":"Focus is over","body":"4 turns done.","tag":"pm-focus"}');
    expect(payload.kind).toBe('focus');
    const plan = planNotification(payload);
    expect(plan.options.data.pane_id).toBeUndefined();
    expect(clickIntent('', plan.options.data)).toMatchObject({ type: 'open', paneId: null });
    expect(openUrl({ paneId: null })).toBe('/');
  });
});
