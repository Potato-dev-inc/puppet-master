// Output stress: a reply with emoji, CJK, right-to-left text and combining marks arrives intact in the terminal and in Chat.
import { calm, S, oneTurn, openChat, sendChat, waitStatus } from './_area.mjs';

export const meta = {
  id: 'work.output-unicode-emoji-rtl',
  title: 'Emoji, Japanese, Chinese, Hebrew, Arabic and accented text in a reply arrive intact',
  area: 'work',
  tags: ['work', 'output', 'unicode'],
  edge: true,
  timeout: 90000,
};

const ROWS = [
  'emoji: 🎉 🚀 ✅ done',
  'cjk: 日本語のテスト 你好世界 한국어',
  'rtl: שלום עולם مرحبا بالعالم',
  'marks: café naïve Zoë ñandú',
  'mixed: ok ✓ → ← ≠ ½ © ™',
];

export async function run(t) {
  await calm(t);
  t.fake.script(oneTurn([S.think(300), ...ROWS.map((r) => S.say(r))]));
  const pane = await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Ask for the unicode reply');
  await sendChat(t, 'say it in every script');
  await t.fake.waitEvent('turn_done');
  await waitStatus(t, pane, 'idle', 30000);

  await t.step('The terminal text holds every row exactly');
  const screen = await pane.screen();
  for (const r of ROWS) t.expect.includes(screen, r);

  await t.step('Chat shows every row exactly');
  for (const r of ROWS) await t.expect.text(r);
}
