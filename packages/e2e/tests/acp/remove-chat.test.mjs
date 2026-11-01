// Remove chat for an agent (Settings > Agents > How to run): a small question first, Cancel changes nothing, Remove stops
// the agent's open chat and puts the agent back to the terminal, and doing it twice is fine.
import path from 'node:path';
import { chatPanes, openSettings, reveal, say } from './_area.mjs';
import { startNimbusChat, chatState, LANE_SELECT, selectValue } from './_robust.mjs';

export const meta = {
  id: 'acp.remove-chat',
  title: 'Remove chat for Nimbus: asks first, stops the chat, back to the terminal',
  area: 'acp',
  tags: ['acp', 'labs', 'robust'],
  fresh: true,
  timeout: 180000,
};

const shot = (t, name) => t.page.screenshot(path.join(t.outDir, `${name}.png`));
const REMOVE = 'aria=Remove chat for Nimbus';
const REMOVE_CSS = 'button[aria-label="Remove chat for Nimbus"]';
const SHEET = 'css=[data-testid=remove-chat-dialog]';

// The Settings page keeps filling in while it opens and can scroll back to its top: look, scroll, look again.
async function showRemove(t) {
  await t.expect.eventually(async () => {
    await reveal(t, REMOVE_CSS);
    return t.page.has(`css=${REMOVE_CSS}`);
  }, { message: 'the Remove chat button for Nimbus is on screen' });
}

export async function run(t) {
  const pane = await startNimbusChat(t);
  await say(t, 'stream 3');
  await t.expect.text('w2');

  await t.step('Settings > Agents shows Remove chat for Nimbus');
  await openSettings(t, 'Agents');
  await reveal(t, LANE_SELECT);
  await shot(t, '1-agents-remove-chat');

  await t.step('The sheet asks first; Cancel changes nothing');
  await showRemove(t);
  await t.click(REMOVE);
  await t.waitFor(SHEET);
  await t.expect.text('Remove chat for Nimbus?');
  await t.expect.text('New sessions will start in a terminal. Your saved chats stay.');
  await shot(t, '2-remove-chat-sheet');
  await t.click('exact=Cancel', { within: SHEET });
  await t.expect.eventually(async () => !(await t.page.has(SHEET)), { message: 'the sheet is closed' });
  t.expect.equal(await selectValue(t, LANE_SELECT), 'acp', 'still a chat');
  t.expect.equal((await chatState(t, pane.id)).closed, false, 'the chat still runs');

  await t.step('Remove: the chat stops and Nimbus is back to the terminal');
  await showRemove(t);
  await t.click(REMOVE);
  await t.waitFor(SHEET);
  await t.click('exact=Remove', { within: SHEET });
  await t.expect.eventually(async () => !(await t.page.has(SHEET)), { timeout: 30000, message: 'the sheet is closed' });
  await t.expect.text('Chat removed for Nimbus.');
  await t.expect.eventually(async () => (await selectValue(t, LANE_SELECT)) === 'terminal', { message: 'new sessions start in a terminal' });
  await t.expect.eventually(async () => (await chatState(t, pane.id))?.closed === true, { timeout: 30000, message: 'the open chat was stopped' });
  await shot(t, '3-chat-removed');

  await t.step('Doing it again is fine');
  const again = await t.page.eval(`window.__TAURI_INTERNALS__.invoke('acp_chat_remove', { agent: 'nimbus', opts: { remove_addon: true } }).then((v) => ({ ok: true, removal: v.removal }), (e) => ({ ok: false, e: String(e) }))`);
  t.expect.equal(again.ok, true, `a second remove is not an error: ${again.e ?? ''}`);
  t.expect.equal(again.removal.chats_stopped, 0, 'nothing left to stop');
  t.expect.equal((await chatPanes(t)).length, 1, 'the saved conversation stays listed');
}
