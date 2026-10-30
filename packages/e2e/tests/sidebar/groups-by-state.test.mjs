// The sidebar sorts panes by what they need: Needs you, Working, Idle. Each row moves when its pane changes state.
import { calm, askScript, slowScript, groupOfRow, sidebarGroups } from './_area.mjs';

export const meta = {
  id: 'sidebar.groups-by-state',
  title: 'Panes are grouped as Needs you, Working and Idle, and the Inbox counts what waits',
  area: 'sidebar',
  tags: ['smoke', 'status'],
  timeout: 90000,
};

export async function run(t) {
  await calm(t);

  await t.step('An idle agent');
  await t.spawnPane('nimbus', { cwd: t.project('idle-one') });

  await t.step('An agent that works on a long task');
  t.fake.script(slowScript(60000));
  const busy = await t.spawnPane('nimbus', { cwd: t.project('busy-one') });
  await busy.write('bravo');

  await t.step('An agent that asks a question');
  t.fake.script(askScript);
  const asking = await t.spawnPane('nimbus', { cwd: t.project('asking-one') });
  await asking.write('charlie');

  await t.step('Each row sits in its group');
  await t.expect.eventually(async () => (await groupOfRow(t, 'charlie')) === 'Needs you', { message: async () => 'charlie should be under Needs you: ' + JSON.stringify(await sidebarGroups(t)) });
  await t.expect.eventually(async () => (await groupOfRow(t, 'bravo')) === 'Working', { message: async () => 'bravo should be under Working: ' + JSON.stringify(await sidebarGroups(t)) });
  t.expect.equal(await groupOfRow(t, 'Nimbus 1'), 'Idle', 'the untouched agent is idle');
  t.expect.equal((await sidebarGroups(t)).map((g) => g.caption), ['Needs you', 'Working', 'Idle'], 'the groups come in this order');

  await t.step('The Inbox shows how many wait for you');
  await t.expect.text('Needs you');
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-row--flat .ck-count')?.innerText`), '1', 'the Inbox badge says 1');

  await t.step('Answer the question: the row leaves Needs you');
  await t.click('exact=charlie', { within: 'css=.ck-side' });
  await asking.write('y', { enter: false });
  await t.expect.eventually(async () => (await groupOfRow(t, 'charlie')) === 'Idle', { message: 'charlie is idle after the answer', timeout: 15000 });
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-row--flat .ck-count')?.innerText || ''`), '', 'the Inbox badge is gone');
}
