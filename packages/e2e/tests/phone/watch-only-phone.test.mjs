// A phone paired with "watch and answer only" can see the panes but cannot type into one or start one.
import { pairPhone, asPhone, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'phone.watch-only-phone',
  title: 'A "watch and answer only" phone can see panes but cannot type or start anything',
  area: 'phone',
  tags: ['security', 'needs-external'],
  edge: true,
};

export const run = guarded(async (t) => {
  const pane = await t.spawnPane('nimbus');
  const phone = await pairPhone(t, { name: 'Watcher', caps: ['watch', 'answer'] });
  t.expect.equal(phone.reply.status, 200);

  await t.step('It may watch: the pane list is served');
  const list = await asPhone(t, phone, { path: '/panes' });
  t.expect.equal(list.status, 200);
  t.expect.ok(list.body.some((p) => p.id === pane.id), 'it sees the pane');

  await t.step('It may not type into the pane');
  const typed = await asPhone(t, phone, { method: 'POST', path: `/panes/${pane.id}/input`, body: { text: 'hello from the phone', append_newline: true } });
  t.expect.equal(typed.status, 403);
  t.expect.equal(typed.body.code, 'DEVICE_NOT_ALLOWED');

  await t.step('It may not start a new pane');
  const started = await asPhone(t, phone, { method: 'POST', path: '/panes', body: { agent_type: 'nimbus', cwd: t.project('phone-made') } });
  t.expect.equal(started.status, 403);

  await t.step('It may not close the pane');
  const closed = await asPhone(t, phone, { method: 'DELETE', path: `/panes/${pane.id}` });
  t.expect.equal(closed.status, 403);

  await t.step('Nothing reached the agent and the pane is still there');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 0, 'no prompt reached the agent');
  t.expect.equal((await t.panes()).length, 1);
});
