// The New agent page: open it from the sidebar, take the example with Tab, pick the fake agent, press Enter. The agent
// pane starts and the fake agent receives exactly the message that was written.
import path from 'node:path';
import { calm } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';
import { replyScript } from '../_helpers.mjs';

export const meta = {
  id: 'app.new-agent-page-sends',
  title: 'The New agent page starts the picked agent with the written message',
  area: 'app',
  tags: ['new-agent', 'spawn'],
};

const INPUT = 'aria=What should the agent do?';

export async function run(t) {
  await calm(t);
  t.fake.script(replyScript('hello from the new agent page'));

  await t.step('Open the New agent page from the sidebar');
  await t.click('css=[data-testid="side-new-agent"]');
  await t.expect.text('What’s up next?');
  await t.expect.text('Add card');

  await t.page.screenshot(path.join(t.outDir, 'new-agent-page-empty.png'));

  await t.step('Tab takes the example');
  await t.click(INPUT);
  const example = await t.page.eval(`document.querySelector('textarea[aria-label="What should the agent do?"]').placeholder`);
  t.expect.ok(example.length > 10, 'an example task is shown as the placeholder');
  await t.press('Tab');
  await t.expect.value(INPUT, example);

  await t.step('Pick the fake agent');
  await t.click('aria=Agent:');
  await t.page.screenshot(path.join(t.outDir, 'new-agent-page-agent-menu.png'));
  await t.click('Nimbus', { within: 'css=[role=menu]' });
  await t.expect.text('Nimbus');

  await t.step('Write the message');
  await t.page.fill(INPUT, 'say hello please');
  await t.page.screenshot(path.join(t.outDir, 'new-agent-page.png'));

  await t.step('Enter sends it');
  await t.click(INPUT);
  await t.press('Enter');

  await t.step('The agent starts and gets exactly that message');
  await t.expect.count(PANE_ROW, 1);
  const prompt = await t.fake.waitEvent('prompt');
  t.expect.equal(prompt.text, 'say hello please', 'the agent received the written message');
  await t.expect.text('hello from the new agent page');
  const panes = await t.panes();
  t.expect.equal(panes[0].agent_type, 'nimbus', 'it is the picked agent');
}
