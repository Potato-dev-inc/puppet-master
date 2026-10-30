// A long task with accents, symbols and non-Latin text reaches the agent whole, and the pane is named after its start.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.unicode-long-task',
  title: 'A long task with accents and symbols reaches the agent whole',
  area: 'runs',
  tags: ['mcp'],
  edge: true,
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('uni');
  const m = mcp(t);
  const task = 'Übersetze "Grüße" & prüfe <b>Café</b> 日本語 ✓ ' + 'wort '.repeat(80) + 'ENDE';

  await t.step('Start a run with a 400-character task full of symbols');
  const started = await m.run({ agent_type: 'nimbus', project_path: project, task });
  t.expect.equal(started.status, 200);

  await t.step('The agent received the whole text, once');
  const got = await t.fake.waitEvent((e) => e.ev === 'prompt' && String(e.text).includes('ENDE'));
  t.expect.equal(got.text.replace(/\s+/g, ' ').trim(), task.replace(/\s+/g, ' ').trim());
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 1);

  await t.step('The pane in the sidebar is named after the start of the task');
  await t.expect.visible('Übersetze', { within: 'css=.ck-side' });

  await t.step('The run completes');
  t.expect.equal(firstRun(await m.wait(project, started.body.handle)).status, 'completed');
});
