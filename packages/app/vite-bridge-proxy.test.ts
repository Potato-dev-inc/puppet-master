// @vitest-environment node
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer, type ViteDevServer } from 'vite';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { bridgeProxyConfig, readBridgeTarget, resolveBridgePortFile } from './vite-bridge-proxy';

describe('resolveBridgePortFile (mirrors src-tauri app_paths.rs)', () => {
  const APP = 'com.puppetmaster.app';
  const FILE = 'puppet-master.bridge.port';

  it('uses PUPPET_MASTER_BRIDGE_PORT_FILE first', () => {
    const env = { PUPPET_MASTER_BRIDGE_PORT_FILE: '/x/custom.port', PUPPET_MASTER_PROFILE: 'maya', APPDATA: 'C:\\A' };
    expect(resolveBridgePortFile(env, 'win32', '/home/u')).toBe('/x/custom.port');
  });

  it('ignores an empty override', () => {
    const env = { PUPPET_MASTER_BRIDGE_PORT_FILE: '  ', APPDATA: join(tmpdir(), 'roaming') };
    expect(resolveBridgePortFile(env, 'win32', '/home/u')).toBe(join(tmpdir(), 'roaming', APP, FILE));
  });

  it('default profile: <APPDATA>/com.puppetmaster.app on Windows', () => {
    const file = resolveBridgePortFile({ APPDATA: join(tmpdir(), 'roaming') }, 'win32', '/home/u');
    expect(file).toBe(join(tmpdir(), 'roaming', APP, FILE));
  });

  it('default profile: unset, empty, "default" and invalid names all mean the default app', () => {
    const base = { APPDATA: join(tmpdir(), 'r') };
    const want = join(tmpdir(), 'r', APP, FILE);
    for (const p of [undefined, '', '  ', 'default', 'DEFAULT', 'bad name', '-x', 'a/b', 'x'.repeat(33)]) {
      expect(resolveBridgePortFile({ ...base, PUPPET_MASTER_PROFILE: p }, 'win32', '/h')).toBe(want);
    }
  });

  it('named profile: <app id>.profile-<name>, trimmed and lowercased', () => {
    const env = { APPDATA: join(tmpdir(), 'r'), PUPPET_MASTER_PROFILE: ' Maya-2 ' };
    expect(resolveBridgePortFile(env, 'win32', '/h')).toBe(join(tmpdir(), 'r', `${APP}.profile-maya-2`, FILE));
  });

  it('falls back to the home folder when APPDATA is unset on Windows', () => {
    expect(resolveBridgePortFile({}, 'win32', join(tmpdir(), 'h'))).toBe(join(tmpdir(), 'h', APP, FILE));
  });

  it('macOS and Linux use their usual app data roots', () => {
    const home = join(tmpdir(), 'h');
    expect(resolveBridgePortFile({}, 'darwin', home)).toBe(join(home, 'Library', 'Application Support', APP, FILE));
    expect(resolveBridgePortFile({}, 'linux', home)).toBe(join(home, '.local', 'share', APP, FILE));
  });
});

type Upstream = { server: http.Server; port: number; seen: http.IncomingMessage[]; upgrades: http.IncomingMessage[] };

function startUpstream(label: string): Promise<Upstream> {
  return new Promise((resolve) => {
    const seen: http.IncomingMessage[] = [];
    const upgrades: http.IncomingMessage[] = [];
    const server = http.createServer((req, res) => {
      seen.push(req);
      res.setHeader('content-type', 'application/json');
      res.end(JSON.stringify({ from: label, url: req.url }));
    });
    server.on('upgrade', (req, socket) => {
      upgrades.push(req);
      socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nhello-from-${label}`);
      socket.end();
    });
    server.listen(0, '127.0.0.1', () => resolve({ server, port: (server.address() as AddressInfo).port, seen, upgrades }));
  });
}

/** A free loopback port nothing listens on (bind, read, release). */
function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, '127.0.0.1', () => {
      const port = (s.address() as AddressInfo).port;
      s.close(() => resolve(port));
    });
  });
}

function get(port: number, path: string): Promise<{ status: number; type: string; body: string }> {
  return new Promise((resolve, reject) => {
    http
      .get({ host: '127.0.0.1', port, path, agent: false }, (res) => {
        let body = '';
        res.setEncoding('utf8');
        res.on('data', (c) => (body += c));
        res.on('end', () => resolve({ status: res.statusCode ?? 0, type: String(res.headers['content-type'] ?? ''), body }));
      })
      .on('error', reject);
  });
}

function wsHandshake(port: number, path: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1', () => {
      socket.write(
        `GET ${path} HTTP/1.1\r\nHost: 127.0.0.1:${port}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n` +
          'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n',
      );
    });
    let data = '';
    socket.setEncoding('utf8');
    socket.on('data', (c) => (data += c));
    socket.on('close', () => resolve(data));
    socket.on('error', reject);
    setTimeout(() => socket.destroy(), 4000);
  });
}

describe('/bridge proxy through a real Vite dev server', () => {
  let dir: string;
  let portFile: string;
  let vite: ViteDevServer;
  let vitePort: number;
  const upstreams: Upstream[] = [];
  const savedEnv = process.env.PUPPET_MASTER_BRIDGE_PORT_FILE;

  beforeEach(async () => {
    dir = mkdtempSync(join(tmpdir(), 'pm-vite-proxy-'));
    portFile = join(dir, 'puppet-master.bridge.port');
    process.env.PUPPET_MASTER_BRIDGE_PORT_FILE = portFile;
    vite = await createServer({
      root: dir,
      configFile: false,
      logLevel: 'silent',
      appType: 'custom',
      optimizeDeps: { noDiscovery: true },
      server: { port: 0, host: '127.0.0.1', hmr: false, watch: null, proxy: bridgeProxyConfig },
    });
    await vite.listen();
    vitePort = (vite.httpServer!.address() as AddressInfo).port;
  });

  afterEach(async () => {
    await vite.close();
    for (const u of upstreams.splice(0)) await new Promise((r) => u.server.close(r));
    if (savedEnv === undefined) delete process.env.PUPPET_MASTER_BRIDGE_PORT_FILE;
    else process.env.PUPPET_MASTER_BRIDGE_PORT_FILE = savedEnv;
    rmSync(dir, { recursive: true, force: true });
  });

  async function upstream(label: string) {
    const u = await startUpstream(label);
    upstreams.push(u);
    return u;
  }

  it('readBridgeTarget reads the port file on every call', () => {
    writeFileSync(portFile, '127.0.0.1:41001\n');
    expect(readBridgeTarget()).toBe('http://127.0.0.1:41001');
    writeFileSync(portFile, '41002');
    expect(readBridgeTarget()).toBe('http://127.0.0.1:41002');
  });

  it('follows the port file: the second request goes to the new port', async () => {
    const a = await upstream('a');
    const b = await upstream('b');
    writeFileSync(portFile, `127.0.0.1:${a.port}\n`);
    const first = await get(vitePort, '/bridge/health?x=1');
    expect(first.status).toBe(200);
    expect(JSON.parse(first.body)).toEqual({ from: 'a', url: '/health?x=1' });

    writeFileSync(portFile, `127.0.0.1:${b.port}\n`);
    const second = await get(vitePort, '/bridge/panes');
    expect(second.status).toBe(200);
    expect(JSON.parse(second.body)).toEqual({ from: 'b', url: '/panes' });
    expect(a.seen).toHaveLength(1);
    expect(b.seen).toHaveLength(1);
  });

  it('marks every proxied call with X-PM-Proxied: 1 (remote, never loopback-trusted)', async () => {
    const a = await upstream('a');
    writeFileSync(portFile, `127.0.0.1:${a.port}\n`);
    await get(vitePort, '/bridge/health');
    expect(a.seen[0].headers['x-pm-proxied']).toBe('1');
  });

  it('answers 502 BRIDGE_UNREACHABLE (JSON) when nothing listens', async () => {
    const dead = await freePort();
    writeFileSync(portFile, `127.0.0.1:${dead}\n`);
    const res = await get(vitePort, '/bridge/health');
    expect(res.status).toBe(502);
    expect(res.type).toContain('application/json');
    const body = JSON.parse(res.body);
    expect(body.code).toBe('BRIDGE_UNREACHABLE');
    expect(body.message).toContain(`port ${dead}`);
    expect(body.message).toContain(portFile);
  });

  it('recovers once the app starts listening on the port named in the file', async () => {
    const dead = await freePort();
    writeFileSync(portFile, `127.0.0.1:${dead}\n`);
    expect((await get(vitePort, '/bridge/health')).status).toBe(502);
    const live = await upstream('live');
    writeFileSync(portFile, `127.0.0.1:${live.port}\n`);
    expect((await get(vitePort, '/bridge/health')).status).toBe(200);
  });

  it('resolves the target per WebSocket upgrade too', async () => {
    const a = await upstream('a');
    const b = await upstream('b');
    writeFileSync(portFile, `127.0.0.1:${a.port}\n`);
    expect(await wsHandshake(vitePort, '/bridge/ws')).toContain('hello-from-a');
    writeFileSync(portFile, `127.0.0.1:${b.port}\n`);
    expect(await wsHandshake(vitePort, '/bridge/ws')).toContain('hello-from-b');
    expect(a.upgrades[0].headers['x-pm-proxied']).toBe('1');
    expect(b.upgrades[0].headers['x-pm-proxied']).toBe('1');
  });
});
