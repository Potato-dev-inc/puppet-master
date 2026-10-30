// If the move is refused, the card stays with the reason and nothing is changed.
import { noCompanions, makeCompanion, openCompanion, injectHomeProblem, clearHomeProblem, companionList, openCompanionWithProblem, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.home-problem-move-fails',
  title: 'When "Move to its own folder" is refused the card stays and says why',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const c = await makeCompanion(t);
  try {
    // the real backend answers the move; the companion really does have a folder of its own already, so it says no
    await injectHomeProblem(t, c.id, { place: 'a folder with other projects in it', suggestedFolder: c.folder });
    await openCompanionWithProblem(t, 'a folder with other projects in it');
    await t.expect.text('a folder with other projects in it');

    await t.step('Press the button: the app refuses');
    await t.click('exact=Move Mochi to its own folder');

    await t.step('The card stays and gives the reason');
    await t.expect.text('Mochi already has a folder of its own.');
    await t.expect.visible('exact=Move Mochi to its own folder');
    t.expect.equal((await companionList(t))[0].folder, c.folder, 'the home did not change');
  } finally {
    clearHomeProblem(t);
  }
});
