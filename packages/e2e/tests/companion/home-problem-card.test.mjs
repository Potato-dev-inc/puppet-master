// A companion whose home is too broad shows a card with ONE button; nothing moves until it is pressed.
// The "too broad" finding is injected on the way to the page (see injectHomeProblem in _area.mjs).
import { noCompanions, makeCompanion, openCompanion, injectHomeProblem, clearHomeProblem, openCompanionWithProblem, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.home-problem-card',
  title: 'A companion with a too-broad home shows "Move Mochi to its own folder" and the card goes after the move',
  area: 'companion',
  tags: [],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const c = await makeCompanion(t);
  const suggested = c.folder;
  try {
    await injectHomeProblem(t, c.id, { place: 'your Desktop', suggestedFolder: suggested }, { moveWorks: true });
    await openCompanionWithProblem(t, 'so other agents there think they are');

    await t.step('The card says where the home is and what that does');
    await t.expect.text("Mochi's home is your Desktop, so other agents there think they are Mochi.");
    await t.expect.text('Nothing else in the old folder is touched.');
    await t.expect.visible('exact=Move Mochi to its own folder');

    await t.step('Press the one button');
    await t.click('exact=Move Mochi to its own folder');

    await t.step('The card is gone and the companion is still here');
    await t.expect.notVisible('exact=Move Mochi to its own folder');
    await t.expect.noText('so other agents there think they are');
    await t.expect.text('Here. Ask me anything.');
  } finally {
    clearHomeProblem(t);
  }
});
