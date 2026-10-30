// The other half of the loopback rule: trust ends the moment a call looks forwarded or names a foreign Host or Origin. Such a
// call is refused when it has NO secret and no token either (the earlier tests send the secret; here there is none).
import { rawBridge, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'security.remote-without-secret',
  title: 'A forwarded call or a foreign Host or Origin with no secret and no token is refused',
  area: 'security',
  tags: ['security'],
  edge: true,
};

export const run = guarded(async (t) => {
  const pane = await t.spawnPane('nimbus');
  const cases = [
    ['X-Forwarded-For header', { 'X-Forwarded-For': '203.0.113.9' }],
    ['Cf-Connecting-Ip header', { 'Cf-Connecting-Ip': '203.0.113.9' }],
    ['foreign Host', { host: 'evil.example.com' }],
    ['foreign Origin', { origin: 'https://evil.example' }],
    ['Origin null', { origin: 'null' }],
  ];
  for (const [name, headers] of cases) {
    await t.step(`${name}, no secret: the pane list is refused`);
    const r = await rawBridge(t, { path: '/panes', bare: true, headers });
    t.expect.equal(r.status, 401, name);
    t.expect.equal(r.body.error, 'pairing_required');
    t.expect.ok(!r.text.includes(pane.id), 'the answer does not leak a pane');
  }

  await t.step('The same calls that change things are refused too, and nothing changes');
  const typed = await rawBridge(t, { method: 'POST', path: `/panes/${pane.id}/input`, bare: true, headers: { 'X-Forwarded-For': '203.0.113.9' }, body: { text: 'x', append_newline: true } });
  t.expect.equal(typed.status, 401);
  const started = await rawBridge(t, { method: 'POST', path: '/panes', bare: true, headers: { origin: 'https://evil.example' }, body: { agent_type: 'nimbus', cwd: t.project('nope') } });
  t.expect.equal(started.status, 401);
  t.expect.equal((await t.panes()).length, 1, 'still one pane');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 0, 'nothing was typed');

  await t.step('And plain loopback without a secret is still served (the designed trust)');
  t.expect.equal((await rawBridge(t, { path: '/panes', bare: true })).status, 200);
});
