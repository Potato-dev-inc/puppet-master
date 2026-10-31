// Every early feature starts switched off. Nothing in Labs is on until a person turns it on.
import { isOn, labsOff, openSettings } from '../night/_area.mjs';

export const meta = {
  id: 'labs.all-off-by-default',
  title: 'Every Labs switch starts off',
  area: 'labs',
  tags: ['labs'],
};

const SWITCHES = [
  'Missions (Preview)',
  'Night Shift',
  'Potato Bot',
  'Simple mode: fewer buttons and plain words',
  'Ask bar',
  'Watch GitHub issues',
  'Anyone with write access to the repo',
  'Offer to fix failed CI',
  'Switch to another of your keys when one hits its rate limit (check your provider\'s terms)',
];

export async function run(t) {
  await t.step('Open Settings > General');
  await openSettings(t, 'General');
  await t.expect.text('Early features');

  await t.step('Each switch is off');
  for (const label of SWITCHES) t.expect.equal(await isOn(t, label), false, `${label} is off`);
}

export const setup = labsOff;
