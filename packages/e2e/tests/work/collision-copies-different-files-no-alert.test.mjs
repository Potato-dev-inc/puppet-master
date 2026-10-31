// Two agents in separate copies of one project change DIFFERENT files: nothing clashes, no alert.
import { calm, gitProject, tauri } from './_area.mjs';
import { turnsScript, ALERT_TITLE, neverShows } from '../collision/_area.mjs';

export const meta = {
  id: 'work.collision-copies-different-files-no-alert',
  title: 'Two agents in separate copies changing different files raise no alert',
  area: 'work',
  tags: ['work', 'collision'],
  edge: true,
  timeout: 180000,
};

export async function run(t) {
  await calm(t);
  const project = gitProject(t, 'copies', { 'one.txt': 'one\n', 'two.txt': 'two\n' });
  t.fake.script(turnsScript([
    { prompt: 'edit alpha', think: 4000, writes: [['one.txt', 'alpha was here']], hold: 60000, say: 'alpha done' },
    { prompt: 'edit beta', think: 4000, writes: [['two.txt', 'beta was here']], hold: 60000, say: 'beta done' },
  ]));

  await t.step('Start two agents, each in its own separate copy of the project');
  const ids = [];
  for (let i = 0; i < 2; i++) {
    const id = await tauri(t, 'new_pane', { agentId: 'nimbus', accountId: null, isolation: 'worktree', projectPath: project });
    ids.push(id);
    await t.expect.eventually(async () => (await t.panes()).find((p) => p.id === id)?.status === 'idle', { timeout: 30000, message: 'the copy agent is ready' });
  }

  await t.step('One edits one.txt, the other two.txt');
  await t.bridge.post(`/panes/${ids[0]}/input`, { text: 'edit alpha', append_newline: true });
  await t.bridge.post(`/panes/${ids[1]}/input`, { text: 'edit beta', append_newline: true });
  await t.fake.waitEvent((e) => e.ev === 'write_file' && e.path === 'two.txt', { timeout: 30000 });

  await t.step('The Inbox stays empty');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
  await neverShows(t, ALERT_TITLE, 35000);
  await t.expect.noText('both changed');
}
