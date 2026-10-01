#!/usr/bin/env node
// Safe stdio integration smoke test for the packaged Rust MCP binary.
// The local mock bridge records requests and never launches a terminal worker.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const cargo = join(process.env.USERPROFILE ?? '', '.cargo', 'bin', process.platform === 'win32' ? 'cargo.exe' : 'cargo');
const binary = process.env.PUPPET_MASTER_TEST_MCP_BINARY ?? join(root, 'packages', 'app', 'src-tauri', 'target', 'debug', process.platform === 'win32' ? 'puppet-master-mcp.exe' : 'puppet-master-mcp');
const temp = mkdtempSync(join(tmpdir(), 'pm-operation-smoke-'));
const portFile = join(temp, 'bridge.port');
const project = join(temp, 'project');
const requests = [];
let operation;
let dispatchCount = 0;
let waitRequests = 0;
let server;
let client;

function buildBinary() {
  if (process.env.PUPPET_MASTER_TEST_MCP_BINARY) {
    assert.ok(existsSync(binary), 'explicit test MCP binary missing');
    return;
  }
  assert.ok(existsSync(cargo), `cargo missing: ${cargo}`);
  const result = spawnSync(cargo, ['build', '--bin', 'puppet-master-mcp'], {
    cwd: join(root, 'packages', 'app', 'src-tauri'), stdio: 'inherit', windowsHide: true,
  });
  assert.equal(result.status, 0, 'Rust MCP binary build failed');
  assert.ok(existsSync(binary), 'built Rust MCP binary missing');
}

function send(proc, message) { proc.stdin.write(`${JSON.stringify(message)}\n`); }

async function rpc(proc, messages, id, method, params) {
  send(proc, { jsonrpc: '2.0', id, method, params });
  const started = Date.now();
  while (Date.now() - started < 10_000) {
    const item = messages.find((entry) => entry.id === id);
    if (item) return item;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timed out waiting for ${method}`);
}

async function main() {
  buildBinary();
  server = createServer(async (req, res) => {
    let raw = '';
    for await (const chunk of req) raw += chunk;
    const body = raw ? JSON.parse(raw) : undefined;
    requests.push({ method: req.method, url: req.url, body, session: req.headers['x-puppet-master-session'] });
    assert.ok(req.headers['x-puppet-master-session'], 'bridge requests need a connection session');
    if (req.method === 'POST' && req.url === '/mcp/mode') {
      const payload = JSON.stringify({ mode: body.mode, tool_count: body.mode === 'agent' ? 9 : 55 });
      res.writeHead(200, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(payload) });
      res.end(payload);
      return;
    }
    if (req.method === 'POST' && req.url === '/agents/run') {
      const payload = JSON.stringify({ handle: 'agent-smoke', operation_id: 'agent-smoke', status: 'completed', verified: true, result: 'mock result', revision: 2, duration_ms: 1 });
      res.writeHead(200, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(payload) });
      res.end(payload);
      return;
    }
    if (req.method === 'GET' && req.url === '/mcp/tools') {
      const tools = ['delegate_work', 'get_operation', 'wait_for_operation', 'cancel_operation'].map((name) => ({
        name, description: name, inputSchema: { type: 'object', properties: {}, required: name === 'delegate_work' ? ['acceptance_criteria'] : [] },
        visibility: { external_mcp: true }, method: name === 'get_operation' ? 'GET' : 'POST',
        path: name === 'delegate_work' ? '/operations/delegate' : `/operations/{operation_id}${name === 'get_operation' ? '' : `/${name === 'wait_for_operation' ? 'wait' : 'cancel'}`}`,
      }));
      const output = JSON.stringify(tools);
      res.writeHead(200, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(output) });
      res.end(output);
      return;
    }
    let status = 200;
    let value;
    if (req.method === 'POST' && req.url === '/operations/delegate') {
      dispatchCount += 1;
      if (body.idempotency_key === 'approval-case') {
        status = 409;
        value = { code: 'APPROVAL_REQUIRED', message: 'human approval required', recoverable: true, retry_after_ms: 0, context: { status: 'waiting_input', source: 'inferred' } };
      } else {
        operation ??= {
          operation_id: 'op-smoke', project_path: project, pane_id: null,
          status: 'running', revision: 3, source: 'native', stage: 'running',
          progress_pct: null, result: null, error: null,
        };
        value = operation;
      }
    } else if (req.method === 'GET' && req.url.startsWith('/operations/op-missing')) {
      status = 404;
      value = { code: 'OPERATION_NOT_FOUND', message: 'operation not found', recoverable: false, context: { operation_id: 'op-missing' } };
    } else if (req.method === 'GET' && req.url.startsWith('/operations/op-smoke')) {
      value = operation;
    } else if (req.method === 'POST' && req.url.startsWith('/operations/op-smoke/wait')) {
      waitRequests += 1;
      if (req.url.includes('project_path=')) {
        const revision = Number(body.after_revision ?? 0) + 1;
        const isFinal = revision >= 5;
        value = { snapshot: { ...operation, status: isFinal ? 'waiting_input' : 'running', revision, stage: isFinal ? 'approval' : 'running' }, reason: isFinal ? 'revision_changed' : 'revision_changed' };
      } else {
        await new Promise((resolve) => setTimeout(resolve, Math.min(Number(body.timeout_ms ?? 200), 200)));
        value = { snapshot: { ...operation, status: 'running', revision: 3 }, reason: 'timeout' };
      }
    } else if (req.method === 'POST' && req.url.startsWith('/operations/op-smoke/cancel')) {
      value = { ...operation, status: 'cancelled', revision: 5 };
    } else {
      status = 404;
      value = { code: 'OPERATION_NOT_FOUND', message: 'operation not found', recoverable: false, context: {} };
    }
    const output = JSON.stringify(value);
    res.writeHead(status, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(output) });
    res.end(output);
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  writeFileSync(portFile, `127.0.0.1:${address.port}\n`);

  client = spawn(binary, [], { cwd: root, env: { ...process.env, PUPPET_MASTER_BRIDGE_PORT_FILE: portFile }, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  let pending = Buffer.alloc(0);
  let stderr = '';
  const messages = [];
  client.stderr.on('data', (chunk) => { stderr += chunk.toString(); });
  client.stdout.on('data', (chunk) => {
    pending = Buffer.concat([pending, chunk]);
    while (true) {
      const newline = pending.indexOf('\n');
      if (newline < 0) break;
      const line = pending.subarray(0, newline).toString('utf8').trim();
      pending = pending.subarray(newline + 1);
      if (line) messages.push(JSON.parse(line));
    }
  });
  const init = await rpc(client, messages, 1, 'initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'operation-smoke', version: '1' } });
  assert.ok(init.result);
  send(client, { jsonrpc: '2.0', method: 'notifications/initialized', params: {} });
  const agentListing = await rpc(client, messages, 20, 'tools/list', {});
  assert.equal(agentListing.result.tools.length, 9);
  assert.ok(agentListing.result.tools.some((tool) => tool.name === 'run_agent'));
  const hidden = await rpc(client, messages, 22, 'tools/call', { name: 'bridge_health', arguments: {} });
  assert.equal(hidden.result.isError, true);
  assert.equal(hidden.result.structuredContent.code, 'MODE_MISMATCH');
  const ran = await rpc(client, messages, 23, 'tools/call', { name: 'run_agent', arguments: { task: 'mock only', wait_ms: 0 } });
  assert.ok(ran.result?.structuredContent, JSON.stringify(ran));
  assert.equal(ran.result.structuredContent.handle, 'agent-smoke');
  assert.equal(ran.result.structuredContent.verified, true);
  const shellMode = await rpc(client, messages, 24, 'tools/call', { name: 'set_mode', arguments: { mode: 'shell' } });
  assert.notEqual(shellMode.result.isError, true);
  const shellListing = await rpc(client, messages, 25, 'tools/list', {});
  assert.ok(shellListing.result.tools.some((tool) => tool.name === 'shell_exec'));
  assert.ok(!shellListing.result.tools.some((tool) => tool.name === 'run_agent'));
  const switched = await rpc(client, messages, 21, 'tools/call', { name: 'set_mode', arguments: { mode: 'both' } });
  assert.equal(switched.result.isError, undefined);
  assert.ok(messages.some((item) => item.method === 'notifications/tools/list_changed'));
  const listing = await rpc(client, messages, 2, 'tools/list', {});
  const names = listing.result.tools.map((tool) => tool.name);
  for (const name of ['delegate_work', 'get_operation', 'wait_for_operation', 'cancel_operation']) assert.ok(names.includes(name), `${name} missing from MCP discovery`);
  assert.ok(listing.result.tools.find((tool) => tool.name === 'delegate_work').inputSchema.required.includes('acceptance_criteria'));

  const args = { project_path: project, task: 'Safe mock-only smoke task', idempotency_key: 'same-request' };
  const delegated = await rpc(client, messages, 3, 'tools/call', { name: 'delegate_work', arguments: args });
  assert.equal(delegated.result.structuredContent.operation_id, 'op-smoke');
  await rpc(client, messages, 4, 'tools/call', { name: 'delegate_work', arguments: args });
  assert.equal(dispatchCount, 2, 'both retries reach the mock bridge');
  assert.deepEqual(requests.filter((item) => item.url === '/operations/delegate').map((item) => item.body.idempotency_key), ['same-request', 'same-request']);

  const got = await rpc(client, messages, 5, 'tools/call', { name: 'get_operation', arguments: { operation_id: 'op-smoke' } });
  assert.equal(got.result.structuredContent.revision, 3);
  const waitParams = { _meta: { progressToken: 'progress-smoke' }, name: 'wait_for_operation', arguments: { project_path: project, operation_id: 'op-smoke', after_revision: 3, until: ['waiting_input'], timeout_ms: 1000 } };
  const waitReq = { jsonrpc: '2.0', id: 6, method: 'tools/call', params: waitParams };
  send(client, waitReq);
  const progressStarted = Date.now();
  while (!messages.some((item) => item.method === 'notifications/progress' && item.params?.progressToken === 'progress-smoke') && Date.now() - progressStarted < 5000) await new Promise((resolve) => setTimeout(resolve, 10));
  const progress = messages.find((item) => item.method === 'notifications/progress' && item.params?.progressToken === 'progress-smoke');
  assert.ok(progress, 'wait should stream progress when a progress token is supplied');
  while (messages.filter((item) => item.method === 'notifications/progress' && item.params?.progressToken === 'progress-smoke').length < 2 && Date.now() - progressStarted < 5000) await new Promise((resolve) => setTimeout(resolve, 10));
  const progressEvents = messages.filter((item) => item.method === 'notifications/progress' && item.params?.progressToken === 'progress-smoke');
  assert.ok(progressEvents.length >= 2, 'wait should expose intermediate revisions before terminal/matched state');
  assert.equal(progressEvents[0].params.message.status, 'running');
  assert.equal(progressEvents.at(-1).params.message.status, 'waiting_input');
  const waited = await rpc(client, messages, 6, 'tools/call', waitParams);
  assert.equal(waited.result.structuredContent.reason, 'matched_state');
  assert.equal(waited.result.structuredContent.snapshot.status, 'waiting_input');
  assert.ok(requests.some((item) => item.url.includes(encodeURIComponent(project))), 'project path should be forwarded');

  send(client, { jsonrpc: '2.0', id: 9, method: 'tools/call', params: { name: 'wait_for_operation', arguments: { operation_id: 'op-smoke', after_revision: 3, timeout_ms: 800 } } });
  await new Promise((resolve) => setTimeout(resolve, 40));
  const readStarted = Date.now();
  const concurrentRead = await rpc(client, messages, 10, 'tools/call', { name: 'get_operation', arguments: { operation_id: 'op-smoke', project_path: project } });
  assert.equal(concurrentRead.result.structuredContent.operation_id, 'op-smoke');
  assert.ok(Date.now() - readStarted < 350, 'get_operation should not queue behind a long wait');
  send(client, { jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: 9, reason: 'smoke cancellation' } });
  const cancelledWait = await rpc(client, messages, 9, 'tools/call', { name: 'wait_for_operation', arguments: { operation_id: 'op-smoke', after_revision: 3, timeout_ms: 800 } });
  assert.equal(cancelledWait.result.isError, true);
  assert.equal(cancelledWait.result.structuredContent.code, 'REQUEST_CANCELLED');

  const timeoutWait = await rpc(client, messages, 12, 'tools/call', { name: 'wait_for_operation', arguments: { operation_id: 'op-smoke', after_revision: 3, timeout_ms: 120 } });
  assert.equal(timeoutWait.result.structuredContent.reason, 'timeout', JSON.stringify(timeoutWait));
  assert.equal(timeoutWait.result.structuredContent.snapshot.operation_id, 'op-smoke');
  assert.equal(timeoutWait.result.structuredContent.snapshot.snapshot, undefined, 'timeout result must not nest bridge wait snapshots');

  const missing = await rpc(client, messages, 11, 'tools/call', { name: 'get_operation', arguments: { operation_id: 'op-missing', project_path: project } });
  assert.equal(missing.result.isError, true);
  assert.equal(missing.result.structuredContent.code, 'OPERATION_NOT_FOUND');
  assert.equal(requests.filter((item) => item.url.startsWith('/operations/op-missing')).length, 1, 'not-found must not be retried');
  const cancelled = await rpc(client, messages, 7, 'tools/call', { name: 'cancel_operation', arguments: { operation_id: 'op-smoke', project_path: project } });
  assert.equal(cancelled.result.structuredContent.status, 'cancelled');

  const blocked = await rpc(client, messages, 8, 'tools/call', { name: 'delegate_work', arguments: { ...args, idempotency_key: 'approval-case' } });
  assert.equal(blocked.result.isError, true);
  assert.equal(blocked.result.structuredContent.code, 'APPROVAL_REQUIRED');
  assert.equal(blocked.result.structuredContent.context.status, 'waiting_input');
  assert.match(stderr, /starting/);
  console.log(`MCP smoke passed: default agent catalog, run result, mode changes, discovery, revision wait/progress, concurrency, cancellation, idempotency, typed errors (${waitRequests} wait requests).`);
}

try { await main(); }
finally {
  if (client && client.exitCode === null) { client.kill(); await new Promise((resolve) => client.once('exit', resolve)); }
  if (server) await new Promise((resolve) => server.close(resolve));
  rmSync(temp, { recursive: true, force: true });
}
