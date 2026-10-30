// Review desk: "Ask to fix" puts a draft in the agent's chat box. It is never sent by the desk.
import { gitProject, writingTurn, openDeskWithPane, rowSel, sentPromptTexts } from './_area.mjs';

export const meta = {
  id: 'review.ask-to-fix-draft',
  title: 'Ask to fix opens the agent chat with a draft built from the reasons and sends nothing',
  area: 'review',
  tags: ['review'],
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
  timeout: 90000,
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  await t.step('An agent finishes a turn that changes the package list');
  const cwd = gitProject(t, 'shop', { 'README.md': 'shop\n', 'package.json': '{"name":"shop"}\n' });
  const pane = await writingTurn(t, cwd, 'add a dependency', [['package.json', '{"name":"shop","dependencies":{"left-pad":"1.3.0"}}\n']]);
  const promptsBefore = sentPromptTexts(t).length;

  await t.step('Open the desk, select the row and press F');
  await openDeskWithPane(t, pane);
  await t.click(rowSel(pane));
  await t.press('f');

  await t.step('A small sheet asks for an optional note; add one and put it in the chat');
  await t.waitFor('css=.ck-sheet');
  await t.type('please pin the version');
  await t.click('Put in chat');

  await t.step('The agent chat opens with the draft: the reason and your note, ready for you to send');
  await t.click('exact=Chat');
  await t.waitFor('aria=Message to the agent');
  await t.expect.eventually(async () => {
    const v = await t.page.value('aria=Message to the agent');
    return v.includes('Before I keep this, please fix:') && v.includes('A package list changed: package.json') && v.includes('please pin the version');
  }, { message: 'the draft in the chat box' });

  await t.step('Nothing was sent to the agent');
  t.expect.equal(sentPromptTexts(t).length, promptsBefore);
}
