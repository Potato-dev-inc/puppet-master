// The five steps of the setup sheet, which agents it lists and which it greys out with the reason.
import { noCompanions, openSetup, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.setup-sheet-steps',
  title: 'The setup sheet shows five steps and lists only agents that can chat',
  area: 'companion',
  tags: [],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await openSetup(t);

  await t.step('The five steps are on the sheet');
  for (const s of ['1. Pick a face', '2. Give it a name', '3. Which agent should it use?', '4. How should it talk?', '5. Where does it live?']) {
    await t.expect.text(s);
  }

  await t.step('Eight faces, a default name and the four ways to talk');
  await t.expect.count('css=.ck-companion-face-pick', 8);
  await t.expect.value('css=.ck-companion-input', 'Mochi');
  for (const p of ['Cheerful', 'Calm and short', 'Curious coach', 'Plain and direct']) await t.expect.visible(`exact=${p}`);
  await t.expect.text('A folder of its own, made for it');

  await t.step('The fake agent is listed and can be picked');
  await t.expect.visible('exact=Nimbus');

  await t.step('Agents that cannot take chat messages are greyed out with the reason');
  await t.expect.text("Chat can't send messages to this agent yet.");

  await t.step('A plain shell is no agent to chat with: it is not listed at all');
  await t.expect.notVisible('PowerShell');
  await t.expect.notVisible('Command Prompt');
});
