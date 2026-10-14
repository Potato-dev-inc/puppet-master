// Harness (route `phone`): stands in for the PC's bridge. Replaces fetch for the fake bridge origin
// and EventSource, so the real PwaApp runs in a plain browser. The network stays blocked otherwise.
import { PHONE_AGENT_TYPES, PHONE_BUFFER, PHONE_DIGEST, PHONE_PANES, PHONE_PREVIEWS, PHONE_QUESTION, PHONE_SCREEN_LINES } from './phone-fixtures';
import { DEFAULT_PUBLIC_SETTINGS } from '../lib/bridge-settings';
import { LS_BRIDGE_URL } from '../lib/bridge-url';
import { PANE_CODEX } from './fixtures';
import { PHONE_TEAM_JOB, PHONE_TEAM_OFF, phoneTeamReply } from './phone-team-fixtures';
import type { HarnessScenario } from './types';

export const MOCK_BRIDGE_URL = 'http://127.0.0.1:17321/bridge';

function b64(bytes: Uint8Array): string {
  let s = '';
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s);
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

/** Cards and jobs the example phone removed this page load (the harness has no real PC behind it). */
const gone = new Set<string>();

/** The `/team/*` routes; null when the path is not one of them. */
function teamRoute(method: string, route: string, scenario: HarnessScenario): Response | null {
  const team = scenario.phone?.team;
  if (route === '/team/cards' && method === 'GET') {
    if (!team) return json(PHONE_TEAM_OFF);
    const reply = phoneTeamReply(team);
    return json({ ...reply, cards: reply.cards.filter((c) => !gone.has(c.id)), jobs: reply.jobs.filter((j) => !gone.has(j.id)) });
  }
  const act = /^\/team\/(cards|jobs)\/([^/]+)\/(dismiss|pull|queue|apply|discard)$/.exec(route);
  if (!act || method !== 'POST') return null;
  if (!team) return json({ error: 'not_allowed' }, 403);
  const [, , id, verb] = act;
  if (verb === 'pull') return json({ ok: true, result: 'Pulled feat/invoices into feat/billing.' });
  if (verb === 'queue') return json({ ok: true, result: { ...PHONE_TEAM_JOB, id: 'job-queued', state: 'waiting' } });
  if (verb === 'apply') {
    gone.add(id);
    return json({ ok: true, result: 'feat/billing is now up to date.' });
  }
  gone.add(id);
  return json({ ok: true });
}

/** Answer one bridge request, or null for an unknown route. */
function answer(method: string, path: string, scenario: HarnessScenario): Response | null {
  const phone = scenario.phone ?? {};
  const route = path.split('?')[0];
  const pane = /^\/panes\/([^/]+)\/(buffer|screen|raw|viewport)$/.exec(route);
  if (route === '/health') return json({ ok: true });
  if (route.startsWith('/team/')) return teamRoute(method, route, scenario);
  if (route === '/panes' && method === 'GET') return json(scenario.phonePanes ?? PHONE_PANES);
  if (route === '/pair/me') {
    return json({ local: false, device_id: 'harness-phone', caps: scenario.phoneCaps ?? ['watch', 'answer', 'type', 'control'] });
  }
  if (route === '/previews') return json({ previews: PHONE_PREVIEWS });
  if (route === '/settings') return json({ ...DEFAULT_PUBLIC_SETTINGS });
  if (route === '/questions') return json({ questions: phone.questions === false ? [] : [PHONE_QUESTION] });
  if (route === '/digest') return json(phone.digest === false ? { ...PHONE_DIGEST, empty: true } : PHONE_DIGEST);
  if (route === '/agents/types') return json(PHONE_AGENT_TYPES);
  if (route === '/panes/stop-all') return json({ stopped: [{ pane_id: PANE_CODEX, agent_type: 'codex', cwd: '' }], failed: [] });
  if (pane) {
    const [, id, kind] = pane;
    if (kind === 'buffer') return json({ content: PHONE_BUFFER[id] ?? '' });
    if (kind === 'screen') {
      const bytes = new TextEncoder().encode(PHONE_SCREEN_LINES.join('\r\n'));
      return json({ b64: b64(bytes), cols: 44, rows: 30 });
    }
    if (kind === 'raw') return json({ data: [] });
    return json({ ok: true, leased: true, held: true, cols: 44, rows: 30 });
  }
  return null;
}

/** An event stream that stays open and delivers the scenario's events once. */
class MockEventSource extends EventTarget {
  readyState = 1;
  url: string;
  static pending: { name: string; data: unknown }[] = [];
  constructor(url: string | URL) {
    super();
    this.url = String(url);
    window.setTimeout(() => {
      this.dispatchEvent(new Event('open'));
      for (const ev of MockEventSource.pending) {
        this.dispatchEvent(new MessageEvent(ev.name, { data: JSON.stringify(ev.data) }));
      }
    }, 50);
  }
  close() {
    this.readyState = 2;
  }
}

export function installMockBridge(scenario: HarnessScenario): void {
  const blocked = window.fetch.bind(window);
  window.fetch = async (input, init) => {
    const href = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    if (href.startsWith(MOCK_BRIDGE_URL)) {
      const method = (init?.method ?? 'GET').toUpperCase();
      return answer(method, href.slice(MOCK_BRIDGE_URL.length), scenario) ?? json({ error: 'harness: not mocked' }, 404);
    }
    return blocked(input, init);
  };
  MockEventSource.pending = scenario.phone?.events ?? [];
  window.EventSource = MockEventSource as unknown as typeof EventSource;
  try {
    localStorage.setItem(LS_BRIDGE_URL, MOCK_BRIDGE_URL);
    if (scenario.phone?.activeTab) localStorage.setItem('pm.mobile.activeTab', scenario.phone.activeTab);
    else localStorage.removeItem('pm.mobile.activeTab');
  } catch {
    /* storage unavailable */
  }
  const ua = scenario.phone?.userAgent;
  if (ua) Object.defineProperty(navigator, 'userAgent', { get: () => ua, configurable: true });
}
