// Branch from here: the Branch button on a finished chat turn opens the sheet; after starting, both chats say so.
import { copyPane, installAgent, emberTurn, openChatTab, selectByTitle, openTab, waitSavePoint, projectDir, gitProject, turnsScript, waitIdle } from './_area.mjs';

export const meta = {
  id: 'branch.chat-button-and-banners',
  title: 'Branch on a finished chat turn starts a copy; the new chat says where it came from and the old one lists it',
  area: 'branch',
  tags: ['branch'],
  timeout: 120000,
};

export async function run(t) {
  await installAgent(t, { id: 'ember', label: 'Ember', transcript: true });
  const cwd = gitProject(t, 'notes', { 'README.md': 'notes\n' });
  t.fake.script(turnsScript([{ prompt: 'Add a greeting', think: 6000, writes: [['greeting.txt', 'hello']], say: 'Added greeting.txt' }]));

  await t.step('An agent with a chat finishes one turn that changed a file');
  const { pane } = await emberTurn(t, { cwd, prompt: 'Add a greeting', answer: 'Added greeting.txt' });
  await waitSavePoint(t, pane, 'after');
  await t.click('css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)', { nth: 0 });
  await openChatTab(t);

  await t.step('Press Branch on the end line of the turn');
  await t.click('exact=Branch');
  await t.expect.text('Branch from here');
  await t.expect.text('Save point: After turn 1');

  await t.step('Switch to Before this turn: the save point changes');
  await t.click('exact=Before this turn');
  await t.expect.text('Save point: Before turn 1');
  await t.click('exact=After this turn');
  await t.expect.text('Save point: After turn 1');

  await t.step('Say what the new agent should try and start it');
  await t.fill('css=.ck-sheet textarea', 'try a shorter greeting');
  await t.click('exact=Start Ember');

  await t.step('A new agent starts in a copy; the old chat now lists its branch');
  await t.expect.text('Branches:', { timeout: 30000 });

  await t.step('Open the new agent from that row: its chat says it was branched from the first one');
  const copy = await copyPane(t);
  await t.page.c.send('Page.navigate', { url: `http://tauri.localhost/index.html?cockpit=1&select=${copy.id}` });
  await t.waitFor('exact=Chat');
  await openChatTab(t);
  await t.expect.text('Branched from Ember · After turn 1');
  await t.expect.visible('exact=Show brief');
  await t.expect.visible('exact=Open original');

  await t.step('Show brief lists what the new agent was told');
  await t.click('exact=Show brief');
  await t.expect.text('Branch: you are starting a new line of work from an earlier point in a chat.');
  await t.expect.text('try a shorter greeting');
}
