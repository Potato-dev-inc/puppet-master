// The New agent page's home cards: pin two, see them after a reload (they are saved in settings), remove one.
import path from 'node:path';
import { calm } from './_area.mjs';

export const meta = {
  id: 'app.new-agent-page-cards',
  title: 'Cards pinned on the New agent page stay after a reload and can be removed',
  area: 'app',
  tags: ['new-agent'],
  fresh: true, // the cards are saved in settings: start from a clean app and leave none behind
};

export async function run(t) {
  await calm(t);

  await t.step('Open the page: no cards yet, no Customize link');
  await t.click('css=[data-testid="side-new-agent"]');
  await t.expect.text('Add card');
  await t.expect.noText('Customize');

  await t.step('Pin Running agents');
  await t.click('Add card');
  await t.click('Running agents', { within: 'css=[role=menu]' });
  await t.expect.text('Nothing is running.');

  await t.step('Pin a saved prompt');
  await t.click('Add card');
  await t.click('A saved prompt', { within: 'css=[role=menu]' });
  await t.page.fill('aria=Saved prompt text', 'Keep commit messages short.');
  await t.click('exact=Pin');
  await t.expect.text('Keep commit messages short.');
  await t.page.screenshot(path.join(t.outDir, 'new-agent-page-cards.png'));

  await t.step('After a reload the cards are still there');
  await t.page.reload();
  await t.expect.text('Inbox');
  await t.press('Ctrl+N'); // the keyboard shortcut opens the page too
  await t.expect.text('Add card');
  await t.expect.text('Nothing is running.');
  await t.expect.text('Keep commit messages short.');

  await t.step('Clicking the saved prompt puts it in the message box');
  await t.click('css=.ck-na-prompt');
  await t.expect.value('aria=What should the agent do?', 'Keep commit messages short.');

  await t.step('Customize shows Remove; removing a card saves');
  await t.click('exact=Customize');
  await t.click('aria=Remove Running agents');
  await t.expect.noText('Nothing is running.');
  await t.click('exact=Done');
  await t.page.reload();
  await t.expect.text('Inbox');
  await t.press('Ctrl+N');
  await t.expect.text('Keep commit messages short.');
  await t.expect.noText('Nothing is running.');

  await t.step('Remove the last card so none is left behind');
  await t.click('exact=Customize');
  await t.click('aria=Remove A saved prompt');
  await t.expect.noText('Keep commit messages short.');
}
