// Designed behaviour (the bridge auth invariant): a plain loopback caller is trusted, so a call from this computer with no secret
// or a wrong one is served (the secret is only logged). The refusal side - forwarder headers, a foreign Host or Origin -
// is pinned in remote-without-secret.test.mjs.
import { rawBridge } from '../companion/_area.mjs';

export const meta = {
  id: 'security.local-call-without-secret',
  title: 'A call from this computer without the secret is served: loopback is trusted by design',
  area: 'security',
  tags: ['security'],
  edge: true,
};

export async function run(t) {
  await t.step('The bridge is up and answers /health to anyone');
  const health = await rawBridge(t, { path: '/health', bare: true });
  t.expect.equal(health.status, 200);
  t.expect.equal(health.body.ok, true);

  await t.step('With the right secret the pane list is served');
  t.expect.equal((await rawBridge(t, { path: '/panes' })).status, 200);

  await t.step('Without any secret the same call is still served (logged, not refused)');
  const none = await rawBridge(t, { path: '/panes', bare: true });
  t.expect.equal(none.status, 200, 'served without a secret (plain loopback is trusted)');

  await t.step('With a wrong secret too');
  const wrong = await rawBridge(t, { path: '/panes', bare: true, headers: { 'x-pm-local-secret': 'not-the-secret' } });
  t.expect.equal(wrong.status, 200, 'served with a wrong secret');

  await t.step('The secret is never in what the bridge says about itself');
  for (const p of ['/health', '/panes', '/mcp/session', '/pair/me']) {
    const r = await rawBridge(t, { path: p });
    t.expect.ok(!r.text.includes(t.app.secret), `${p} does not echo the secret`);
  }
}
