// An idempotency key makes a repeated run_agent the same run: the client may retry after a timeout without a second pane.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'runs.same-key-same-run',
  title: 'Sending the same run twice with one idempotency key makes one run and one pane',
  area: 'runs',
  tags: ['mcp'],
  edge: true,
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('retry');
  const m = mcp(t);
  const body = { agent_type: 'nimbus', project_path: project, task: 'job with a key', idempotency_key: 'e2e-retry-key-1' };

  await t.step('Send the request');
  const first = await m.run(body);
  t.expect.equal(first.status, 200);

  await t.step('Send the very same request again, as a client does after a timeout');
  const second = await m.run(body);
  t.expect.equal(second.status, 200);
  t.expect.equal(second.body.handle, first.body.handle, 'the same handle comes back');

  await t.step('One pane, one agent process, one answer');
  const done = firstRun(await m.wait(project, first.body.handle));
  t.expect.equal(done.status, 'completed');
  await t.expect.count(PANE_ROW, 1);
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'start').length, 1, 'the agent was started once');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 1, 'the agent got the task once');

  await t.step('A different key is a different run');
  const other = await m.run({ ...body, idempotency_key: 'e2e-retry-key-2' });
  t.expect.ok(other.body.handle !== first.body.handle, 'a new handle');
});
