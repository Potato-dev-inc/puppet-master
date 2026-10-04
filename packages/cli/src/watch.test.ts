/**
 * `puppet-master watch` contract tests against a mock bridge that behaves like the real
 * `/operations` routes: GET snapshot, long-poll wait with `after_revision` / `until` /
 * `timeout_ms`, 404 for unknown ids. The mock can be stopped and restarted on another port to
 * model a bridge restart.
 */
import assert from 'node:assert/strict';
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, it } from 'node:test';
import { runWatch, type WatchLine } from './watch.js';

interface Op {
  operation_id: string;
  project_path: string;
  status: string;
  revision: number;
  result: string | null;
  error?: unknown;
  agent_run_id?: string;
}

class MockBridge {
  ops = new Map<string, Op>();
  waits: Array<{ id: string; after_revision: number }> = [];
  getCount = 0;
  sessionHeaders: Array<string | undefined> = [];
  failNextWaitsWith = 0;
  private server?: Server;
  private sockets = new Set<import('node:net').Socket>();
  private waiters = new Set<() => void>();
  port = 0;

  constructor(private portFile: string) {}

  add(id: string, status = 'running', extra: Partial<Op> = {}): Op {
    const op: Op = {
      operation_id: id,
      project_path: '/proj',
      status,
      revision: 1,
      result: null,
      ...extra,
    };
    this.ops.set(id, op);
    return op;
  }

  set(id: string, patch: Partial<Op>): void {
    const op = this.ops.get(id)!;
    Object.assign(op, patch, { revision: op.revision + 1 });
    for (const wake of [...this.waiters]) wake();
  }

  async start(port = 0): Promise<void> {
    this.server = createServer((req, res) => void this.handle(req, res));
    this.server.on('connection', (socket) => {
      this.sockets.add(socket);
      socket.on('close', () => this.sockets.delete(socket));
    });
    await new Promise<void>((resolve) => this.server!.listen(port, '127.0.0.1', resolve));
    this.port = (this.server.address() as { port: number }).port;
    writeFileSync(this.portFile, `127.0.0.1:${this.port}\n`);
  }

  async stop(): Promise<void> {
    for (const socket of this.sockets) socket.destroy();
    for (const wake of [...this.waiters]) wake();
    await new Promise<void>((resolve) => (this.server ? this.server.close(() => resolve()) : resolve()));
    this.server = undefined;
  }

  private json(res: ServerResponse, status: number, body: unknown): void {
    res.writeHead(status, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(body));
  }

  private async handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
    this.sessionHeaders.push(req.headers['x-puppet-master-session'] as string | undefined);
    const url = new URL(req.url ?? '/', 'http://x');
    const parts = url.pathname.split('/').filter(Boolean);
    let raw = '';
    for await (const chunk of req) raw += chunk;
    const body = raw ? JSON.parse(raw) : {};
    const notFound = () =>
      this.json(res, 404, { error: 'nope', message: 'nope', code: 'OPERATION_NOT_FOUND' });

    if (parts[0] !== 'operations') return notFound();
    if (parts[1] === 'by-handle' && req.method === 'GET') {
      const handle = url.searchParams.get('agent_run_id');
      const op = [...this.ops.values()].find((o) => o.agent_run_id === handle);
      return op ? this.json(res, 200, op) : notFound();
    }
    const op = this.ops.get(parts[1]);
    if (req.method === 'GET' && parts.length === 2) {
      this.getCount += 1;
      return op ? this.json(res, 200, op) : notFound();
    }
    if (req.method === 'POST' && parts[2] === 'wait') {
      if (!op) return notFound();
      this.waits.push({ id: op.operation_id, after_revision: body.after_revision });
      if (this.failNextWaitsWith > 0) {
        this.failNextWaitsWith -= 1;
        return this.json(res, 503, { code: 'STATE_UNAVAILABLE', message: 'busy', recoverable: true });
      }
      const until: string[] = body.until ?? ['completed', 'failed', 'cancelled'];
      const deadline = Date.now() + (body.timeout_ms ?? 30_000);
      const ready = () => op.revision > body.after_revision && until.includes(op.status);
      while (!ready() && Date.now() < deadline) {
        await new Promise<void>((resolve) => {
          const timer = setTimeout(done, Math.max(1, deadline - Date.now()));
          const waiter = () => done();
          function done() {
            clearTimeout(timer);
            resolve();
          }
          this.waiters.add(waiter);
          setTimeout(() => this.waiters.delete(waiter), Math.max(1, deadline - Date.now()) + 5);
        });
        if (res.destroyed || res.socket?.destroyed) return;
      }
      return this.json(res, 200, { snapshot: op, reason: ready() ? 'status' : 'timeout' });
    }
    return notFound();
  }
}

let dir: string;
let bridge: MockBridge;
const savedEnv = process.env.PUPPET_MASTER_BRIDGE_PORT_FILE;

beforeEach(async () => {
  dir = mkdtempSync(join(tmpdir(), 'pm-watch-test-'));
  const portFile = join(dir, 'bridge.port');
  process.env.PUPPET_MASTER_BRIDGE_PORT_FILE = portFile;
  bridge = new MockBridge(portFile);
  await bridge.start();
});

afterEach(async () => {
  await bridge.stop();
  if (savedEnv === undefined) delete process.env.PUPPET_MASTER_BRIDGE_PORT_FILE;
  else process.env.PUPPET_MASTER_BRIDGE_PORT_FILE = savedEnv;
  rmSync(dir, { recursive: true, force: true });
});

async function watch(
  ids: string[],
  extra: { timeoutMs?: number; stream?: boolean } = {},
): Promise<{ code: number; lines: WatchLine[] }> {
  const lines: WatchLine[] = [];
  const code = await runWatch({
    ids,
    projectPath: '/proj',
    timeoutMs: extra.timeoutMs ?? 10_000,
    stream: extra.stream ?? false,
    out: (line) => lines.push(JSON.parse(line) as WatchLine),
    retryBaseMs: 10,
    retryMaxMs: 40,
  });
  return { code, lines };
}

const later = (ms: number, fn: () => void | Promise<void>) =>
  new Promise<void>((resolve) =>
    setTimeout(async () => {
      await fn();
      resolve();
    }, ms),
  );

describe('puppet-master watch', () => {
  it('blocks until completed, prints exactly one JSON result and exits 0', async () => {
    bridge.add('op-1');
    const done = later(120, () => bridge.set('op-1', { status: 'completed', result: 'all done' }));
    const { code, lines } = await watch(['op-1']);
    await done;
    assert.equal(code, 0);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].status, 'completed');
    assert.equal(lines[0].result, 'all done');
    assert.equal(lines[0].operation_id, 'op-1');
  });

  it('does not poll: an unchanged operation costs one long-poll, not repeated status checks', async () => {
    bridge.add('op-1');
    const done = later(300, () => bridge.set('op-1', { status: 'completed', result: 'r' }));
    await watch(['op-1']);
    await done;
    assert.equal(bridge.waits.length, 1);
    assert.equal(bridge.getCount, 1, 'one initial resolve, no status polling');
  });

  it('prints an already-terminal operation immediately without waiting', async () => {
    bridge.add('op-1', 'completed', { result: 'earlier' });
    const { code, lines } = await watch(['op-1']);
    assert.equal(code, 0);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].result, 'earlier');
    assert.equal(bridge.waits.length, 0);
  });

  it('exits 1 with the failed terminal result and its error', async () => {
    bridge.add('op-1');
    const done = later(50, () =>
      bridge.set('op-1', { status: 'failed', error: { code: 'ACCEPTANCE_FAILED', message: 'x' } }),
    );
    const { code, lines } = await watch(['op-1']);
    await done;
    assert.equal(code, 1);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].status, 'failed');
    assert.deepEqual((lines[0].error as { code: string }).code, 'ACCEPTANCE_FAILED');
  });

  it('prints a cancelled terminal result on cancellation and exits 1', async () => {
    bridge.add('op-1');
    const done = later(50, async () => {
      bridge.set('op-1', { status: 'cancelling' });
      await new Promise((r) => setTimeout(r, 30));
      bridge.set('op-1', { status: 'cancelled' });
    });
    const { code, lines } = await watch(['op-1']);
    await done;
    assert.equal(code, 1);
    assert.equal(lines.length, 1, 'cancelling is not terminal; only the cancelled result prints');
    assert.equal(lines[0].status, 'cancelled');
  });

  it('exits 2 and reports waiting_input without claiming a terminal result', async () => {
    bridge.add('op-1');
    const done = later(50, () => bridge.set('op-1', { status: 'waiting_input' }));
    const { code, lines } = await watch(['op-1']);
    await done;
    assert.equal(code, 2);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].status, 'waiting_input');
  });

  it('stops at the deadline with a timeout result instead of hanging', async () => {
    bridge.add('op-1');
    const started = Date.now();
    const { code, lines } = await watch(['op-1'], { timeoutMs: 400 });
    assert.ok(Date.now() - started < 3_000, 'returned promptly after the deadline');
    assert.equal(code, 3);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].status, 'timeout');
    assert.equal(lines[0].last_status, 'running');
    assert.equal(lines[0].operation_id, 'op-1');
  });

  it('reconnects after the bridge restarts and keeps waiting on the same operation', async () => {
    bridge.add('op-1');
    const restart = later(150, async () => {
      await bridge.stop();
      await new Promise((r) => setTimeout(r, 100));
      await bridge.start(); // new port, port file rewritten
    });
    const done = later(600, () => bridge.set('op-1', { status: 'completed', result: 'survived' }));
    const { code, lines } = await watch(['op-1']);
    await restart;
    await done;
    assert.equal(code, 0);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].result, 'survived');
    assert.ok(bridge.waits.length >= 1, 'resumed waiting after reconnect');
    assert.ok(
      bridge.waits.every((w) => w.id === 'op-1'),
      'never switches operation',
    );
  });

  it('resumes from the last seen revision after a reconnect', async () => {
    bridge.add('op-1');
    const first = later(60, () => bridge.set('op-1', { status: 'waiting_input' }));
    // waiting_input ends the first watch; start a second watcher that rides out a restart.
    await watch(['op-1']);
    await first;
    bridge.set('op-1', { status: 'running' });
    const restart = later(100, async () => {
      await bridge.stop();
      await bridge.start();
    });
    const done = later(400, () => bridge.set('op-1', { status: 'completed', result: 'r' }));
    const { code } = await watch(['op-1']);
    await restart;
    await done;
    assert.equal(code, 0);
    const afterRestart = bridge.waits.at(-1)!;
    assert.ok(afterRestart.after_revision >= 3, `resumed from revision, got ${afterRestart.after_revision}`);
  });

  it('survives a bridge that is down at start and comes up later', async () => {
    bridge.add('op-1', 'completed', { result: 'ok' });
    await bridge.stop();
    rmSync(join(dir, 'bridge.port'));
    const up = later(150, () => bridge.start());
    const { code, lines } = await watch(['op-1']);
    await up;
    assert.equal(code, 0);
    assert.equal(lines[0].result, 'ok');
  });

  it('reports a timeout (not a crash) when the bridge never comes back', async () => {
    bridge.add('op-1');
    await bridge.stop();
    const { code, lines } = await watch(['op-1'], { timeoutMs: 300 });
    assert.equal(code, 3);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].status, 'timeout');
    assert.equal(lines[0].bridge_unreachable, true);
  });

  it('retries transient 5xx replies from the bridge', async () => {
    bridge.add('op-1');
    bridge.failNextWaitsWith = 2;
    const done = later(150, () => bridge.set('op-1', { status: 'completed', result: 'r' }));
    const { code } = await watch(['op-1']);
    await done;
    assert.equal(code, 0);
  });

  it('exits 4 immediately for an unknown operation instead of retrying', async () => {
    const started = Date.now();
    const { code, lines } = await watch(['missing']);
    assert.equal(code, 4);
    assert.ok(Date.now() - started < 2_000);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].status, 'error');
    assert.equal((lines[0].error as { code: string }).code, 'OPERATION_NOT_FOUND');
  });

  it('resolves an agent_run handle through by-handle', async () => {
    bridge.add('op-9', 'completed', { result: 'via handle', agent_run_id: 'run-abc' });
    const { code, lines } = await watch(['run-abc']);
    assert.equal(code, 0);
    assert.equal(lines[0].operation_id, 'op-9');
  });

  it('needs no MCP connection ownership: requests carry no session header', async () => {
    bridge.add('op-1');
    const done = later(50, () => bridge.set('op-1', { status: 'completed', result: 'r' }));
    await watch(['op-1']);
    await done;
    assert.ok(bridge.sessionHeaders.length > 0);
    assert.ok(bridge.sessionHeaders.every((header) => header === undefined));
  });

  it('prints one result per operation when watching several, in the order given', async () => {
    bridge.add('a');
    bridge.add('b');
    const done = Promise.all([
      later(120, () => bridge.set('a', { status: 'completed', result: 'A' })),
      later(40, () => bridge.set('b', { status: 'failed' })),
    ]);
    const { code, lines } = await watch(['a', 'b']);
    await done;
    assert.equal(code, 1);
    assert.deepEqual(
      lines.map((line) => [line.operation_id, line.status]),
      [
        ['a', 'completed'],
        ['b', 'failed'],
      ],
    );
  });

  it('--stream prints each change once and still ends with every final state', async () => {
    bridge.add('op-1');
    const done = later(80, () => bridge.set('op-1', { status: 'completed', result: 'r' }));
    const { code, lines } = await watch(['op-1'], { stream: true });
    await done;
    assert.equal(code, 0);
    assert.equal(lines.filter((line) => line.status === 'completed').length, 1);
  });
});
