// An approval menu with three options: "Always allow in this project" answers the agent with that choice.
import { script } from './agent-asks-menu.test.mjs';
import { openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-approval-allow-always',
  title: 'Always allow in this project answers the agent and it carries on',
  area: 'chat',
  tags: ['questions', 'approval', 'known-bug'],
  // Bug: the card offers this button, but the app refuses the answer and shows the raw text
  // `INVALID_PROMPT_CHOICE: "Always allow in this project" is a broad approval; it needs allow_broad`. The agent is not answered.
  // Run it anyway with E2E_RUN_KNOWN_BUGS=1.
  skip: 'The "Always allow" button on a generic numbered approval is refused by the app (INVALID_PROMPT_CHOICE ... needs allow_broad) and the agent is never answered',
};

export async function run(t) {
  if (meta.skip && !process.env.E2E_RUN_KNOWN_BUGS) t.skip(meta.skip);
  t.fake.script(script);
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'please run the command');

  await t.step('The card shows the command and three choices');
  await t.expect.text('Answer the agent');
  await t.expect.visible('exact=Allow once');
  await t.expect.visible('exact=Always allow in this project');
  await t.expect.visible('exact=Deny');

  await t.step('Choose Always allow');
  await t.click('exact=Always allow in this project');

  await t.step('The agent carries on and the card is gone');
  await t.fake.waitEvent('approval_answer');
  await t.expect.text('the command ran');
  await t.expect.notVisible('exact=Always allow in this project');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'approval_answer').length, 1, 'one answer reached the agent');
}
