// Edge case: two panes are open one after the other. What is typed into one never shows up in the other.
import { calm, openPane, focusTerminal } from './_area.mjs';

export const meta = {
  id: 'panes.two-panes-separate-input',
  title: 'Typing in one pane does not reach the other pane',
  area: 'panes',
  tags: ['terminal'],
  edge: true,
};

const terminalText = (t) => t.page.eval(`document.querySelector('.ck-term-slot')?.innerText || ''`);

export async function run(t) {
  await calm(t);
  const a = await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  const b = await t.spawnPane('nimbus', { cwd: t.project('beta') });

  await t.step('Type into the first pane');
  await openPane(t, 'Nimbus 1');
  await focusTerminal(t);
  await t.type('only for alpha');
  await t.press('Enter');
  await t.fake.waitEvent((e) => e.ev === 'prompt' && e.text === 'only for alpha');

  await t.step('Open the second pane and type something else');
  await openPane(t, 'Nimbus 2');
  await t.expect.text('NIMBUS 0.4.2');
  t.expect.ok(!(await terminalText(t)).includes('only for alpha'), 'the second terminal does not show the first message');
  await focusTerminal(t);
  await t.type('only for beta');
  await t.press('Enter');
  await t.fake.waitEvent((e) => e.ev === 'prompt' && e.text === 'only for beta');

  await t.step('Each agent got only its own message');
  const prompts = t.fake.events().filter((e) => e.ev === 'prompt').map((e) => e.text);
  t.expect.equal(prompts, ['only for alpha', 'only for beta'], 'two prompts, in order');
  t.expect.ok(!(await a.screen()).includes('only for beta'), 'the first agent never saw the second message');
  t.expect.ok(!(await b.screen()).includes('only for alpha'), 'the second agent never saw the first message');
}
