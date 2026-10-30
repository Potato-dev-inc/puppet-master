// The phone card reads GET /night: a redacted copy of the night. No project name, no folder path, no pane id.
import { gitProject, nightSetup, startOneTaskNight, writerScript } from './_area.mjs';

export const meta = {
  id: 'night.bridge-summary-redacted',
  title: 'The night summary the phone reads has no project names, folders or pane ids',
  area: 'night',
  tags: ['night', 'privacy'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await t.step('No night yet: the phone view is empty');
  let res = await t.bridge.get('/night');
  t.expect.ok([200, 404].includes(res.status), `an empty night answers 200 or 404 (${res.status})`);

  await t.step('Run a night in a project with a recognisable name');
  const { dir } = await startOneTaskNight(t, { text: 'phone redaction task', name: 'very-private-project-name', script: writerScript('x.txt', 'x') });
  await t.waitFor('exact=Open Morning', { timeout: 60000 });

  await t.step('The phone summary has the counts and the task words but nothing that identifies the folder');
  await t.expect.eventually(async () => (res = await t.bridge.get('/night')).status === 200, { message: 'the summary is served' });
  const text = JSON.stringify(res.body);
  t.expect.equal(res.body.state, 'done');
  t.expect.equal(res.body.done, 1);
  t.expect.match(res.body.line, /^1 of 1 done/);
  for (const secret of ['very-private-project-name', dir, dir.replace(/\\/g, '/'), 'pm-e2e', 'Users']) {
    t.expect.ok(!text.includes(secret), `the summary does not contain "${secret}"`);
  }
  const task = res.body.tasks[0];
  t.expect.equal(task.project, null);
  t.expect.equal(task.paneId, null);
  t.expect.equal(task.summaries, []);
}

export const setup = nightSetup;
