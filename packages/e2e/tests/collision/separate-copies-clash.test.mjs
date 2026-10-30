// Collision Alert: two agents in separate copies of one project change the same line; the card names the file and the line.
import { gitProject, turnsScript, tauri } from './_area.mjs';

export const meta = {
  id: 'collision.separate-copies-clash',
  title: 'Two agents in separate copies changing the same line of one file raise a clash card naming the file and line',
  area: 'collision',
  tags: ['collision'],
  timeout: 150000,
};

export async function run(t) {
  const file = 'app.txt';
  const base = ['one', 'two', 'three', 'four', 'five'].join('\n') + '\n';
  const project = gitProject(t, 'app', { [file]: base });
  const edit = (word) => ['one', 'two', word, 'four', 'five'].join('\n') + '\n';
  t.fake.script(turnsScript([
    { prompt: 'edit alpha', think: 4000, writes: [[file, edit('alpha line')]], hold: 90000, say: 'alpha done' },
    { prompt: 'edit beta', think: 4000, writes: [[file, edit('beta line')]], hold: 90000, say: 'beta done' },
  ]));

  await t.step('Start two agents, each in its own separate copy of the project');
  const ids = [];
  for (let i = 0; i < 2; i++) {
    const id = await tauri(t, 'new_pane', { agentId: 'nimbus', accountId: null, isolation: 'worktree', projectPath: project });
    ids.push(id);
    await t.expect.eventually(async () => (await t.panes()).find((p) => p.id === id)?.status === 'idle', { timeout: 30000, message: 'the copy agent is ready' });
  }
  const copies = (await t.panes()).filter((p) => ids.includes(p.id)).map((p) => p.cwd);
  t.expect.ok(copies.every((c) => /worktrees/.test(c)), `both agents work in copies: ${JSON.stringify(copies)}`);
  t.expect.equal(new Set(copies).size, 2, 'two different copies');

  await t.step('Ask each one to change line 3 of app.txt');
  await t.bridge.post(`/panes/${ids[0]}/input`, { text: 'edit alpha', append_newline: true });
  await t.bridge.post(`/panes/${ids[1]}/input`, { text: 'edit beta', append_newline: true });

  await t.step('The Inbox card says they both changed app.txt, in separate copies, on the same line');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('both changed app.txt', { timeout: 120000 });
  await t.expect.text('In separate copies.');
  await t.expect.eventually(async () => /Line 3 overlaps\.|Lines 3-3 overlap\./.test(await t.page.text()), { message: 'the card names line 3' });
  await t.expect.text('Nothing was paused. You decide.');
}
