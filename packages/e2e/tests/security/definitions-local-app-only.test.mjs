// Agent definitions (approve, reload, remove...) are the app's own business: the local app may, an MCP session may not,
// and a forwarded request may not either.
import { rawBridge, mcp, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'security.definitions-local-app-only',
  title: 'An MCP session or a remote caller cannot change agent definitions; the app can',
  area: 'security',
  tags: ['security'],
  edge: true,
};

export const run = guarded(async (t) => {
  await t.step('The app itself (no MCP session, from this computer) may reload the definitions');
  const own = await rawBridge(t, { method: 'POST', path: '/agents/definitions/reload', body: {} });
  t.expect.equal(own.status, 200);

  await t.step('An MCP session is refused: AUTHORIZATION_DENIED');
  const m = mcp(t);
  for (const action of ['reload', 'approve', 'revoke', 'remove', 'restore']) {
    const r = await m.call('POST', `/agents/definitions/${action}`, { file: 'nimbus.toml', fingerprint: 'sha256:0000' });
    t.expect.equal(r.status, 403, `${action} by an MCP session`);
    t.expect.equal(r.body.code, 'AUTHORIZATION_DENIED');
  }

  await t.step('A forwarded request is refused too');
  const remote = await rawBridge(t, { method: 'POST', path: '/agents/definitions/reload', body: {}, headers: { 'X-Forwarded-For': '203.0.113.9' } });
  t.expect.equal(remote.status, 401);

  await t.step('Nothing changed: the fake agent is still approved and listed');
  const list = await rawBridge(t, { path: '/agents/definitions' });
  t.expect.includes(JSON.stringify(list.body), 'nimbus');
  t.expect.equal((await t.spawnPane('nimbus')).agent, 'nimbus');
});
