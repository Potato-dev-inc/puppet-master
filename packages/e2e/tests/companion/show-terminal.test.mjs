// "Show terminal" opens the companion's pane in the normal pane view, with what the agent has on screen.
import { noCompanions, makeCompanion, openCompanion, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.show-terminal',
  title: "Show terminal opens the companion's own pane",
  area: 'companion',
  tags: [],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);

  await t.step('Press Show terminal');
  await t.click('exact=Show terminal');

  await t.step("The agent's screen is there");
  await t.expect.text('NIMBUS 0.4.2');
  await t.expect.text('read SOUL.md, USER.md and MEMORY.md');
});
