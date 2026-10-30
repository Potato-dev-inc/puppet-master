// Pick for me: the first row of the New agent menu suggests an agent from your own history and says why.
// It only suggests: nothing starts until you press Start, and this test never presses it.
import { projectDir, selectByTitle, runTurn, SLOW_REPLY, counted } from './_area.mjs';

export const meta = {
  id: 'scores.pick-for-me-is-only-a-suggestion',
  title: 'Pick for me names an agent with one plain sentence, keeps the choice yours and starts nothing',
  area: 'scores',
  tags: ['scores', 'pick-for-me'],
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
  timeout: 90000,
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  t.fake.script(SLOW_REPLY);
  const cwd = projectDir(t, 'pick');
  const pane = await t.spawnPane('nimbus', { cwd });
  await runTurn(t, pane, 'first task');
  await counted(t, cwd, 1);
  await selectByTitle(t, 'first task');
  const panes = (await t.panes()).length;

  await t.step('Open the New agent menu: the first row is Pick for me');
  await t.click('aria=New agent');
  await t.waitFor('Pick for me');

  await t.step('Press Pick for me: one sentence says which agent and why');
  await t.expect.eventually(async () => !(await t.page.text()).includes('Looking...'), { message: 'Pick for me has finished looking' });
  await t.click('Pick for me');
  await t.waitFor('exact=Why this one');
  await t.expect.eventually(async () => /Too early to tell your agents apart here|looks? about the same|Only .* is ready|so your usual one|so the first one ready/i.test(await t.page.text()), { message: 'a plain sentence about the choice' });

  await t.step('Why this one lists what was looked at, and that it stays on this computer');
  await t.click('Why this one');
  await t.expect.text('Worked out on your computer. Nothing you type is sent anywhere.');

  await t.step('Nothing was started');
  t.expect.equal((await t.panes()).length, panes);

  await t.step('Back to the list of agents: nothing was started');
  await t.click(await t.page.has('exact=Choose myself') ? 'exact=Choose myself' : 'exact=Back');
  await t.expect.visible('exact=Nimbus');
  t.expect.equal((await t.panes()).length, panes);
}
