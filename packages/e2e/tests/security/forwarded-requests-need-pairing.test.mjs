// A request that reaches the bridge through a tunnel or proxy on this computer carries a header a forwarder adds. Such a
// request is treated as remote: it needs a paired phone's token, even with the right secret.
import { rawBridge } from '../companion/_area.mjs';

export const meta = {
  id: 'security.forwarded-requests-need-pairing',
  title: 'A request that looks forwarded (a tunnel or proxy header) is refused without a paired phone',
  area: 'security',
  tags: ['security'],
  edge: true,
};

const FORWARDER_HEADERS = {
  'X-Forwarded-For': '203.0.113.9',
  'X-Forwarded-Host': 'phone.example.net',
  'X-Real-Ip': '203.0.113.9',
  'Cf-Connecting-Ip': '203.0.113.9',
  'Cdn-Loop': 'cloudflare',
  'Forwarded': 'for=203.0.113.9',
  'X-PM-Proxied': '1',
};

export async function run(t) {
  for (const [name, value] of Object.entries(FORWARDER_HEADERS)) {
    await t.step(`${name} on a pane-list call`);
    const r = await rawBridge(t, { path: '/panes', headers: { [name]: value } });
    t.expect.equal(r.status, 401, `${name} makes the call remote`);
    t.expect.equal(r.body.error, 'pairing_required');
  }

  await t.step('A made-up bearer token is no better');
  const bad = await rawBridge(t, { path: '/panes', headers: { 'X-Forwarded-For': '203.0.113.9', Authorization: 'Bearer not-a-real-token' } });
  t.expect.equal(bad.status, 401);
  t.expect.equal(bad.body.error, 'invalid_token');

  await t.step('The tool list and the session routes are closed to it as well');
  for (const p of ['/mcp/tools', '/mcp/session', '/agents/types', '/team/cards']) {
    const r = await rawBridge(t, { path: p, headers: { 'X-Forwarded-For': '203.0.113.9' } });
    t.expect.equal(r.status, 401, `${p} needs a paired phone`);
  }

  await t.step('Only /health stays open to a remote caller, and it holds no secret');
  const health = await rawBridge(t, { path: '/health', headers: { 'X-Forwarded-For': '203.0.113.9' } });
  t.expect.equal(health.status, 200);
  t.expect.ok(!health.text.includes(t.app.secret), 'no secret in /health');
}
