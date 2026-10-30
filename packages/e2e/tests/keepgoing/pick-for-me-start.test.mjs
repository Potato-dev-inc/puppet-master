// Start in the Pick for me panel starts the agent it named, in a new pane, and nothing else.
// The suggested agent is made a fake one: a copy of the fake agent named like the real program sits first on the app's PATH.
import { installFakeProgram, openNewMenu, sclick, stable, waitClean } from '../night/_area.mjs';

export const meta = {
  id: 'keepgoing.pick-for-me-start',
  title: 'Start in Pick for me opens the suggested agent in a new pane',
  area: 'keepgoing',
  tags: ['pick-for-me'],
  timeout: 60000,
};

const SCRIPT = (layout) => `agent = "${layout}"
boot_ms = 100
[[turn]]
steps = [ { do = "think", ms = 300 }, { do = "say", text = "hello from the suggested agent" } ]
`;

const FAKEABLE = { 'Claude Code': ['claude', 'claude'], 'Codex CLI': ['codex', 'codex'] }; // name -> [program, screen layout]

export async function run(t) {
  await waitClean(t); // the helper pane the launcher used to open the window
  const before = (await t.panes()).length;

  await t.step('Open Pick for me');
  await openNewMenu(t);
  await sclick(t, 'Pick for me');
  await t.waitFor('css=.ck-pick');
  await stable(t, 'css=.ck-pick-name');
  const name = await t.page.eval(`document.querySelector('.ck-pick-name').innerText`);
  const fake = FAKEABLE[name];
  if (!fake) return t.skip(`the first agent ready on this PC is ${name}; this test can only fake Claude Code or Codex CLI`);

  const undo = installFakeProgram(t, fake[0]);
  try {
    t.fake.script(SCRIPT(fake[1]));

    await t.step(`Press Start ${name}`);
    await t.click(`exact=Start ${name}`);

    await t.step('A pane of that agent opens and the fake agent starts');
    await t.expect.eventually(async () => (await t.panes()).length === before + 1, { message: 'one new pane' });
    t.expect.equal((await t.panes()).filter((p) => p.agent_type === fake[0]).length, 1, 'the new pane is the suggested agent');
    await t.expect.eventually(async () => ['idle', 'running'].includes((await t.panes()).find((p) => p.agent_type === fake[0])?.status), { message: 'the agent pane is up' });
    await t.expect.notVisible('css=.ck-pick');
  } finally {
    undo();
  }
}

export { baseSetup as setup } from '../night/_area.mjs';
