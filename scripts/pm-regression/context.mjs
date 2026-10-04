// Shared run context for pm-regression: CLI arguments, backend selection, the sandbox folder, the
// MCP child process + JSON-RPC client, and the failure/handle bookkeeping every scenario writes to.
// One context object is passed around instead of module globals.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const PONG = 'Reply with only the word PONG. Use no tools.';

// Which CLI proves a backend is installed; used only to choose the default backend list in bench mode.
export const BACKEND_BINARIES = { opencode_native: ['opencode'], cursor_agent: ['cursor-agent', 'agent'], codex: ['codex'], claude: ['claude'] };

export function onPath(names) {
  const exts = process.platform === 'win32' ? (process.env.PATHEXT ?? '.EXE;.CMD;.BAT').split(';').concat('') : [''];
  for (const dir of (process.env.PATH ?? '').split(delimiter)) {
    for (const name of names) for (const ext of exts) if (dir && existsSync(join(dir, name + ext))) return join(dir, name + ext);
  }
  return null;
}

function parseArgs(argv) {
  return Object.fromEntries(
    argv.reduce((pairs, arg, index, all) => {
      if (arg.startsWith('--')) pairs.push([arg.slice(2), all[index + 1]?.startsWith('--') || all[index + 1] === undefined ? true : all[index + 1]]);
      return pairs;
    }, []),
  );
}

/** Build the context from process.argv. Exits with code 2 when the MCP binary is missing. */
export function createContext(argv = process.argv.slice(2)) {
  const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
  const args = parseArgs(argv);
  const bench = Boolean(args.bench || args['dry-run']); // --dry-run implies the bench plan
  const dryRun = Boolean(args['dry-run']);
  const defaultBackends = bench ? Object.keys(BACKEND_BINARIES).filter((name) => onPath(BACKEND_BINARIES[name])) : ['opencode_native', 'cursor_agent'];
  const backends = (args.backends ? String(args.backends).split(',') : defaultBackends).filter(Boolean);
  const binary = String(args.binary ?? join(root, 'packages', 'mcp-server', 'dist', process.platform === 'win32' ? 'puppet-master-mcp.exe' : 'puppet-master-mcp'));
  if (!dryRun && !existsSync(binary)) {
    console.error(`MCP binary not found: ${binary}`);
    process.exit(2);
  }
  // --project reuses an existing folder (e.g. one Cursor already trusts, for visible-pane runs); it is never deleted.
  const ownSandbox = !args.project;
  const sandbox = ownSandbox ? mkdtempSync(join(tmpdir(), 'pm-regression-')) : String(args.project);
  const proc = dryRun ? null : spawn(binary, [], { stdio: ['pipe', 'pipe', 'inherit'], windowsHide: true });

  const failures = [];
  return {
    root, args, bench, dryRun, backends, binary, sandbox, ownSandbox, proc, failures,
    repeat: Number(args.repeat ?? 1),
    only: args.only ? String(args.only).split(',') : null,
    results: [],
    handlesSeen: [],
    ...createRpcClient(proc),
    check(label, condition, detail = '') {
      if (!condition) failures.push(`${label}${detail ? `: ${detail}` : ''}`);
      return Boolean(condition);
    },
  };
}

/** Line-delimited JSON-RPC over the MCP child's stdio. `tool` unwraps a tools/call result. */
function createRpcClient(proc) {
  const pending = new Map();
  let buffer = '';
  let nextId = 1;
  proc?.stdout.on('data', (chunk) => {
    buffer += chunk;
    for (let newline = buffer.indexOf('\n'); newline >= 0; newline = buffer.indexOf('\n')) {
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      if (!line) continue;
      try {
        const message = JSON.parse(line);
        pending.get(message.id)?.(message);
      } catch { /* notification or noise */ }
    }
  });

  function rpc(method, params, timeoutMs = 600_000) {
    const id = nextId++;
    proc.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`timeout: ${method}`)); }, timeoutMs);
      pending.set(id, (message) => { clearTimeout(timer); pending.delete(id); resolve(message); });
    });
  }

  async function tool(name, toolArgs) {
    const response = await rpc('tools/call', { name, arguments: toolArgs });
    const result = response.result;
    if (!result) return { _error: response.error?.message ?? 'no result' };
    const payload = result.structuredContent ?? (() => { try { return JSON.parse(result.content?.[0]?.text ?? '{}'); } catch { return { raw: result.content?.[0]?.text }; } })();
    return result.isError ? { _error: payload?.message ?? payload?.raw ?? 'tool error', ...payload } : payload;
  }

  return { rpc, tool };
}
