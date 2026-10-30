// Clip It: a turn that has not ended cannot be clipped, so its end line (and the button) is not there.
import { installAgent, turnsScript, transcriptLine, openChatTab, projectDir } from './_area.mjs';

export const meta = {
  id: 'clip.no-button-while-running',
  title: 'A turn that has not finished has no Make clip button and the chat menu has no clip entry',
  area: 'clip',
  tags: ['clip'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await installAgent(t, { id: 'ember', label: 'Ember', transcript: true });
  t.fake.script(turnsScript([{ prompt: 'Think about the plan', think: 60000, say: 'finally decided' }]));
  const cwd = projectDir(t, 'busy');

  await t.step('An agent is asked something and is still working on it');
  const pane = await t.spawnPane('ember', { cwd });
  await pane.write('Think about the plan');
  transcriptLine(cwd, { role: 'user', content: 'Think about the plan', ts: Date.now() });
  await t.expect.eventually(async () => (await pane.info())?.status === 'running', { message: 'the agent is working' });
  await t.click('css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)', { nth: 0 });
  await openChatTab(t);
  await t.expect.text('Think about the plan');

  await t.step('The turn is not finished: no Make clip button on it');
  await t.expect.visible('aria=Chat menu');
  await t.expect.notVisible('exact=Make clip');

  await t.step('The chat menu has no clip entry either');
  await t.click('aria=Chat menu');
  await t.expect.visible('exact=Go live…');
  await t.expect.notVisible('Make clip of last turn');
  await t.press('Escape');
}
