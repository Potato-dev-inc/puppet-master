// Edge case: the bridge trusts a plain call from this computer, but a request that carries proxy or foreign-origin headers
// (what a tunnel adds) must pair first. Nothing may be started through such a request.
import { calm } from './_area.mjs';

export const meta = {
  id: 'app.bridge-refuses-forwarded-requests',
  title: 'The bridge refuses requests that arrive through a proxy or from another origin',
  area: 'app',
  tags: ['bridge', 'security'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  const base = `http://127.0.0.1:${t.app.bridgePort}`;
  const call = (headers, method = 'GET', body) => fetch(`${base}/panes`, { method, headers: { ...(body ? { 'content-type': 'application/json' } : {}), ...headers }, body: body && JSON.stringify(body) });

  await t.step('A plain call from this computer is accepted');
  t.expect.equal((await call({})).status, 200, 'plain local call');

  await t.step('A call that carries X-Forwarded-For is refused');
  const forwarded = await call({ 'x-forwarded-for': '203.0.113.9' });
  t.expect.equal(forwarded.status, 401, 'forwarded call');
  t.expect.equal((await forwarded.json()).error, 'pairing_required', 'it asks to pair first');

  await t.step('Even the app key does not help a forwarded call');
  t.expect.equal((await call({ 'x-forwarded-for': '203.0.113.9', 'x-pm-local-secret': t.app.secret })).status, 401, 'forwarded call with the key');

  await t.step('A call from another web origin is refused');
  t.expect.equal((await call({ origin: 'https://evil.example' })).status, 401, 'foreign origin');

  await t.step('A forwarded call cannot start an agent');
  const start = await call({ 'x-forwarded-for': '203.0.113.9' }, 'POST', { agent_type: 'nimbus', cwd: t.project('refused') });
  t.expect.equal(start.status, 401, 'start refused');
  t.expect.equal((await t.panes()).length, 0, 'no pane was started');
  await t.expect.text('No agents running.');
}
