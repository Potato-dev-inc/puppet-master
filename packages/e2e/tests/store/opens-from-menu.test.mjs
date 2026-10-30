// "+" > More agents… opens the Agent Store: a title, a search box, five filter chips and a grid of cards. Back leaves it.
import { openNewMenu, openStore, storeCards, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'store.opens-from-menu',
  title: 'More agents in the plus menu opens the Agent Store with its cards',
  area: 'store',
  tags: ['store', 'smoke'],
  timeout: 90000,
};

export async function run(t) {
  await t.step('Open the plus menu and press More agents');
  await openStore(t);

  await t.step('The page explains itself and offers search and filters');
  await t.expect.text('Agents you can add to Puppet Master. Each card says how well we have tested it.');
  await t.expect.visible('aria=Search agents');
  for (const chip of ['All', 'Ready', 'Open source', 'Needs a key', 'Tested']) await t.expect.visible({ exact: chip, within: 'css=.ck-store-chips' });

  await t.step('The catalog is listed: well-known agents and the rest');
  const cards = await storeCards(t);
  t.expect.ok(cards.length >= 19, `at least 19 agents are listed (found ${cards.length})`);
  for (const name of ['Claude Code', 'Codex CLI', 'OpenCode', 'Gemini CLI', 'Aider', 'Cline', 'Qwen Code']) {
    t.expect.ok(cards.some((c) => c.startsWith(`${name}:`)), `${name} has a card`);
  }

  await t.step('Every card says how well it was tested');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('.ck-store-card')].every((c) => /Tested|Basic support/.test(c.innerText))`), true);

  await t.step('Back returns to the cockpit');
  await t.click('exact=Back');
  await t.expect.notVisible('exact=Agent Store');
  await t.expect.text('No agents running.');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
