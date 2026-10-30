// A pairing code works once. A wrong code, a used code and a half-empty request pair nothing.
import { rawBridge, pairPhone, guarded, invoke, devicePublicKey } from '../companion/_area.mjs';

export const meta = {
  id: 'phone.code-single-use',
  title: 'A pairing code works once; a wrong code or a used code pairs nothing',
  area: 'phone',
  tags: ['security', 'needs-external'],
  edge: true,
};

export const run = guarded(async (t) => {
  const key = devicePublicKey();
  const remote = { 'X-Forwarded-For': '203.0.113.60' };

  await t.step('A code nobody made is refused');
  const wrong = await rawBridge(t, { method: 'POST', path: '/pair', headers: remote, body: { pairing_code: 'ABCD1234', device_name: 'Intruder', device_public_key: key } });
  t.expect.ok(wrong.status >= 400 && wrong.status < 500, `refused (${wrong.status})`);

  await t.step('A request without the phone\'s key or name is refused');
  const half = await rawBridge(t, { method: 'POST', path: '/pair', headers: remote, body: { pairing_code: 'ABCD1234' } });
  t.expect.equal(half.status, 400);

  await t.step('A real code works the first time');
  const phone = await pairPhone(t, { name: 'First phone' });
  t.expect.equal(phone.reply.status, 200);

  await t.step('The same code a second time is refused');
  const again = await rawBridge(t, { method: 'POST', path: '/pair', headers: remote, body: { pairing_code: phone.code, device_name: 'Copycat', device_public_key: key } });
  t.expect.ok(again.status >= 400 && again.status < 500, `refused (${again.status})`);

  await t.step('Only the first phone is paired');
  const devices = (await invoke(t, 'list_paired_mobile_devices')).ok;
  t.expect.equal(devices.map((d) => d.name), ['First phone']);
});
