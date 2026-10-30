// Pick for me: the first row of the plus menu suggests one agent from your own history and says why in one plain sentence.
// Start is never pressed here, so no agent is started. (Which agent is "first ready" depends on the programs on the PC
// running the test, and the sentence changes after a usage limit, so the test checks the shape, not the name.)
import { openNewMenu, sclick, stable } from '../night/_area.mjs';

export const meta = {
  id: 'keepgoing.pick-for-me-panel',
  title: 'Pick for me names one agent, gives one sentence and offers Start or Choose myself',
  area: 'keepgoing',
  tags: ['pick-for-me', 'smoke'],
};

export async function run(t) {
  const before = (await t.panes()).length;
  await t.step('Open the plus menu: Pick for me is the first row');
  await openNewMenu(t);
  const hint = await t.expect.eventually(async () => {
    const h = await t.page.eval(`(() => { const row = [...document.querySelectorAll('[role=menuitem], .ck-menu-item')].find((b) => b.innerText.startsWith('Pick for me')); return row ? row.innerText.replace('Pick for me', '').trim() : null; })()`);
    return h && h !== 'Looking...' ? h : null;
  }, { message: 'the row names an agent' });
  t.log('suggested agent: ' + hint);
  t.expect.ok(hint !== 'None ready', 'an agent is ready on this PC');

  await t.step('Press it: the panel shows the name, one sentence and two buttons');
  await sclick(t, 'Pick for me');
  await t.waitFor('css=.ck-pick');
  await stable(t, 'css=.ck-pick-name');
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-pick-name').innerText`), hint);
  const reason = await t.page.eval(`document.querySelector('.ck-pick-reason').innerText`);
  t.log('reason: ' + reason);
  t.expect.ok(/^[^\n]+\.$/.test(reason.trim()), `the reason is one sentence (${reason})`);
  t.expect.includes(reason, hint, 'the sentence names the suggested agent');
  await t.expect.visible(`exact=Start ${hint}`);
  await t.expect.visible('exact=Choose myself');

  await t.step('Nothing was started');
  t.expect.equal((await t.panes()).length, before, 'no pane was started');
}

export { baseSetup as setup } from '../night/_area.mjs';
