// A very long question with angle brackets and an emoji is shown as plain text and nothing in it runs.
import { askUser, openInbox } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.ask-user-special-text',
  title: 'A long question with HTML-looking text and an emoji shows as plain text',
  area: 'inbox',
  tags: ['inbox', 'ask-user'],
  edge: true,
};

const LONG = `Should I run <img src=x onerror="window.__pwned=1"> and <b>bold</b> ${'very long words '.repeat(40)}end 🚀?`;

export async function run(t) {
  await askUser(t, { question: LONG, choices: [{ id: 'ok', label: 'OK <i>now</i>' }], context: 'a <script>window.__pwned=1</script> note' });
  await openInbox(t);

  await t.step('The text shows as typed');
  await t.expect.text('<img src=x onerror="window.__pwned=1">');
  await t.expect.text('<b>bold</b>');
  await t.expect.text('end 🚀?');
  await t.expect.text('<script>window.__pwned=1</script>');
  await t.expect.visible('exact=OK <i>now</i>');

  await t.step('Nothing in it was run');
  t.expect.equal(await t.page.eval('window.__pwned === undefined'), true, 'no script from the question ran');
  t.expect.equal(await t.page.eval(`document.querySelectorAll('.ck-inbox img, .ck-inbox b, .ck-inbox script').length`), 0, 'no tags were created');
}
