// run_agent says no, with a code and a hint, to a request it cannot do. No pane is opened for a refused run.
import { mcp, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.refused-requests',
  title: 'run_agent refuses an unknown agent, a blank task and read_only on a pane, and opens no pane',
  area: 'runs',
  tags: ['mcp'],
  edge: true,
};

export const run = guarded(async (t) => {
  const project = t.project('refused');
  const m = mcp(t);

  await t.step('An agent type that does not exist');
  const unknown = await m.run({ agent_type: 'no_such_agent', project_path: project, task: 'hello' });
  t.expect.equal(unknown.status, 400);
  t.expect.equal(unknown.body.code, 'UNKNOWN_AGENT_TYPE');
  t.expect.includes(unknown.body.context.valid, 'nimbus', 'the answer lists the valid agent types');

  await t.step('A blank task');
  const blank = await m.run({ agent_type: 'nimbus', project_path: project, task: '   ' });
  t.expect.equal(blank.status, 400);
  t.expect.equal(blank.body.code, 'INVALID_TASK');

  await t.step('read_only on an agent whose pane cannot enforce it');
  const ro = await m.run({ agent_type: 'nimbus', project_path: project, task: 'look around', read_only: true });
  t.expect.equal(ro.status, 400);
  t.expect.equal(ro.body.code, 'READ_ONLY_UNSUPPORTED');

  await t.step('A body that is not JSON');
  const junk = await m.call('POST', '/agents/run', '{ this is not json');
  t.expect.equal(junk.status, 400);

  await t.step('Nothing started: no pane, no agent process');
  t.expect.equal((await t.panes()).length, 0, 'no pane was opened');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'start').length, 0, 'no agent was started');
  await t.expect.text('No agents running.');
});
