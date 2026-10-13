import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  bridgeUrlForPublicUrl,
  pairingLink,
  publicOriginForPublicUrl,
  publicOriginFromTunnelInfo,
} from './pair-link.js';

test('bridge url is the public origin plus /bridge, like the desktop UI', () => {
  assert.equal(bridgeUrlForPublicUrl('https://abc.trycloudflare.com'), 'https://abc.trycloudflare.com/bridge');
  assert.equal(bridgeUrlForPublicUrl('https://abc.trycloudflare.com/'), 'https://abc.trycloudflare.com/bridge');
  assert.equal(bridgeUrlForPublicUrl('https://abc.trycloudflare.com/bridge'), 'https://abc.trycloudflare.com/bridge');
  assert.equal(bridgeUrlForPublicUrl('abc.example.com'), 'https://abc.example.com/bridge');
});

test('public origin strips a trailing /bridge', () => {
  assert.equal(publicOriginForPublicUrl('https://abc.trycloudflare.com/bridge/'), 'https://abc.trycloudflare.com');
});

test('tunnel info prefers the tunnel and ignores loopback addresses', () => {
  assert.equal(
    publicOriginFromTunnelInfo({ tunnelUrl: 'https://x.trycloudflare.com/', bridgeProxyUrl: 'https://x.trycloudflare.com/bridge' }),
    'https://x.trycloudflare.com',
  );
  assert.equal(
    publicOriginFromTunnelInfo({ tunnelUrl: null, bridgeProxyUrl: 'https://proxy.example.com/bridge' }),
    'https://proxy.example.com',
  );
  assert.equal(publicOriginFromTunnelInfo({ tunnelUrl: 'http://127.0.0.1:1420', bridgeProxyUrl: 'http://127.0.0.1:1420/bridge' }), null);
  assert.equal(publicOriginFromTunnelInfo(null), null);
});

test('pairing link is origin plus /pair/<code>', () => {
  assert.equal(pairingLink('https://x.trycloudflare.com/', 'ABCD1234'), 'https://x.trycloudflare.com/pair/ABCD1234');
});
