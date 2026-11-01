// Chat (ACP): turn the Labs switch on, set an agent to Chat, start a session from the + menu, see the streamed text and
// one activity line, answer a permission card with Reject, stop a long turn, open the conversation in a terminal and come
// back, then restart the app and find the conversation again. The agent is the fake program with --acp: no real agent.
import path from 'node:path';
import { chooseOption, chatPanes, say, calm, openSettings, goToPage, clickControl, revealControl, reveal, MESSAGE_BOX } from './_area.mjs';

export const meta = {
  id: 'acp.chat-workspace',
  title: 'A chat session: stream, activity line, permission card, Stop, terminal and back, restart',
  area: 'acp',
  tags: ['acp', 'labs'],
  fresh: true,
  timeout: 240000,
};

const LABS = 'aria=Chat sessions (ACP)';
const LANE_SELECT = 'select[aria-label="How Nimbus runs"]';
const labsBackend = (t) => t.page.eval(`window.__TAURI_INTERNALS__.invoke('agent_lane_get').then((v) => String(v.labs_enabled), () => 'error')`);
const selectValue = (t, selector) => t.page.eval(`document.querySelector(${JSON.stringify(selector)})?.value`);
const shot = (t, name) => t.page.screenshot(path.join(t.outDir, `${name}.png`));
const labsOn = (t) => t.page.eval(`document.querySelector('[role=switch][aria-label="Chat sessions (ACP)"]')?.getAttribute('aria-checked')`);

export async function run(t) {
  await calm(t);

  await t.step('The Chat way is hidden while the Labs switch is off');
  await openSettings(t, 'Agents');
  t.expect.equal(await t.page.has('css=[data-testid=agent-lane]'), false, 'no "How to run" rows while Labs is off');

  await t.step('Turn on Chat sessions (ACP) in General > Labs');
  await goToPage(t, 'General');
  await revealControl(t, LABS);
  t.expect.equal(await labsOn(t), 'false', 'it starts off');
  // The Settings page fills in while it opens, so a press can land on a row that has just moved: look, press, look again.
  for (let attempt = 0; attempt < 4 && (await labsOn(t)) !== 'true'; attempt++) {
    await t.sleep(600);
    await clickControl(t, LABS);
    await t.sleep(900);
  }
  await t.expect.eventually(async () => (await labsOn(t)) === 'true', { message: 'the Labs switch is on' });
  await t.expect.eventually(async () => (await labsBackend(t)) === 'true', { message: 'the app knows the switch is on' });

  await t.step('Agents > How to run: Nimbus runs as a Chat (ACP)');
  await goToPage(t, 'Agents');
  await t.expect.eventually(() => t.page.eval(`!!document.querySelector(${JSON.stringify(LANE_SELECT)})`), { message: 'the How to run row for Nimbus' });
  await reveal(t, LANE_SELECT);
  t.expect.equal(await selectValue(t, LANE_SELECT), 'terminal', 'Terminal is the default');
  await chooseOption(t, LANE_SELECT, 'Chat (ACP)');
  await t.expect.eventually(async () => (await selectValue(t, LANE_SELECT)) === 'acp', { message: 'the choice is saved' });
  await shot(t, '1-settings-how-to-run');

  await t.step('Start Nimbus from the + menu: it is a chat session');
  await t.click('aria=New agent');
  await t.waitFor('css=[data-testid=lane-row-nimbus]');
  await shot(t, '2-new-session-switch');
  await t.click('exact=Nimbus', { within: 'css=[data-testid=lane-row-nimbus]' });
  await t.expect.eventually(async () => (await chatPanes(t)).length === 1, { timeout: 30000, message: 'a chat pane is listed' });
  await t.waitFor('css=[data-testid=pane-lane]');
  await t.waitFor(MESSAGE_BOX);
  const chat = (await chatPanes(t))[0];
  t.expect.equal(chat.agent_type, 'nimbus');

  await t.step('Streamed text');
  await say(t, 'stream 6');
  await t.expect.text('w5');

  await t.step('One activity line for the turn\'s tool calls');
  await say(t, 'work');
  await t.expect.text('All done: two files changed, tests pass.');
  await t.expect.text('Read 2 files');
  await t.expect.text('Edited 2 files');
  await t.expect.text('Ran 1 command');
  await t.click('re=/Read 2 files/');
  await t.expect.text('Edit src/main.rs');
  await shot(t, '3-activity-line');

  await t.step('The agent\'s settings and slash commands');
  await t.waitFor('css=select[aria-label="Model"]');
  await chooseOption(t, 'select[aria-label="Model"]', 'Nimbus Large');
  await t.expect.eventually(async () => (await t.page.value('css=select[aria-label="Model"]')) === 'b', { message: 'the model changed through the agent' });
  await t.page.fill(MESSAGE_BOX, '/');
  await t.waitFor('css=[data-testid=slash-menu]');
  await t.expect.text('Review the changes in this folder');
  await shot(t, '4-slash-menu');
  await t.page.fill(MESSAGE_BOX, '');

  await t.step('A permission card: the agent waits, nobody answers for the person');
  await say(t, 'perm');
  await t.waitFor('css=[data-testid=acp-dock]');
  await t.expect.text('Write notes.txt');
  await t.expect.text('Allow Once');
  await t.expect.text('Let agents answer these questions');
  await shot(t, '5-permission-card');
  await t.click('exact=Reject', { within: 'css=[data-testid=acp-dock]' });
  await t.expect.text('chose:reject_once');
  await t.expect.notVisible('exact=Allow Once');

  await t.step('Stop a long turn');
  await say(t, 'slow');
  await t.waitFor('css=[data-testid=acp-strip]');
  await t.expect.text('s3 ');
  await t.click('exact=Stop', { within: 'css=[data-testid=acp-dock]' });
  await t.expect.eventually(async () => !(await t.page.has('css=[data-testid=acp-strip]')), { timeout: 20000, message: 'the strip is gone once the agent ended the turn' });
  await t.expect.text('interrupted');
  await shot(t, '6-stopped');

  await t.step('Open in terminal: a normal Nimbus pane continues the same conversation');
  await t.click('exact=Open in terminal');
  await t.expect.eventually(async () => (await t.panes()).some((p) => p.lane !== 'acp' && p.agent_type === 'nimbus'), { timeout: 30000, message: 'a terminal pane started' });
  await t.expect.text('NIMBUS 0.4.2');
  await t.waitFor('exact=Back to chat');
  // A terminal that is still starting counts as working (Back to chat would ask first): let it settle.
  await t.expect.eventually(async () => (await t.panes()).some((p) => p.lane !== 'acp' && p.agent_type === 'nimbus' && p.status === 'idle'), { timeout: 20000, message: 'the terminal is idle' });
  await shot(t, '7-in-terminal');

  await t.step('Back to chat: the terminal stops and the chat has the conversation again');
  await t.click('exact=Back to chat');
  await t.expect.eventually(async () => !(await t.panes()).some((p) => p.lane !== 'acp' && p.agent_type === 'nimbus'), { timeout: 30000, message: 'the terminal is gone' });
  await t.waitFor(MESSAGE_BOX);
  await t.expect.text('All done: two files changed, tests pass.');
  await shot(t, '8-back-in-chat');

  await t.step('Restart the app: the conversation is still there');
  const before = (await chatPanes(t))[0].id;
  await t.restartApp();
  await t.expect.eventually(async () => (await chatPanes(t)).length === 1, { timeout: 30000, message: 'the chat pane is listed again' });
  t.expect.equal((await chatPanes(t))[0].id, before, 'the same conversation');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.waitFor(MESSAGE_BOX);
  await t.expect.text('All done: two files changed, tests pass.');
  await t.expect.text('stream 6');
  await shot(t, '9-after-restart');
  await say(t, 'stream 2');
  await t.expect.text('w1');
}
