// The app starts and shows the cockpit: sidebar, inbox and the empty state.
export const meta = {
  id: 'app.opens-cockpit',
  title: 'The cockpit opens with an empty sidebar and an empty inbox',
  area: 'app',
  tags: ['smoke'],
};

export async function run(t) {
  await t.step('The cockpit is on screen');
  await t.expect.text('Puppet Master');
  await t.expect.visible('Inbox');

  await t.step('Nothing is running yet');
  await t.expect.text('No agents running.');
  await t.expect.text('Nothing needs you');

  await t.step('The bottom buttons are there');
  for (const name of ['Set up agents', 'Project', 'Your plans', 'Scores', 'Review', 'Settings']) await t.expect.visible(`exact=${name}`);
}
