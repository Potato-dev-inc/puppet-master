// Local stand-in for the Messages API, so a real Claude Code UI can be driven with no network and no cost.
// First request of a turn -> a Bash tool_use (so Claude draws its permission dialog); after a tool_result -> a text reply.
import http from 'node:http';
const port = Number(process.argv[2] || 18999);
const delayMs = Number(process.argv[3] || 0);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sse = (res, event, data) => res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
const server = http.createServer(async (req, res) => {
  let body = '';
  for await (const c of req) body += c;
  console.log(req.method, req.url, body.length);
  if (req.url.includes('count_tokens')) {
    res.setHeader('content-type', 'application/json');
    return res.end(JSON.stringify({ input_tokens: 12 }));
  }
  if (!req.url.startsWith('/v1/messages')) {
    res.setHeader('content-type', 'application/json');
    return res.end('{}');
  }
  let j = {};
  try { j = JSON.parse(body); } catch {}
  const model = j.model || 'claude-fake';
  const hasToolResult = body.includes('"tool_result"');
  const isMain = Array.isArray(j.tools) && j.tools.length > 3;
  if (!j.stream) {
    res.setHeader('content-type', 'application/json');
    return res.end(JSON.stringify({ id: 'msg_x', type: 'message', role: 'assistant', model, content: [{ type: 'text', text: 'Fake title' }], stop_reason: 'end_turn', usage: { input_tokens: 5, output_tokens: 3 } }));
  }
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  if (isMain && !hasToolResult && delayMs) await sleep(delayMs);
  sse(res, 'message_start', { type: 'message_start', message: { id: 'msg_1', type: 'message', role: 'assistant', model, content: [], stop_reason: null, usage: { input_tokens: 10, output_tokens: 1 } } });
  if (isMain && !hasToolResult) {
    sse(res, 'content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id: 'toolu_fake1', name: 'Bash', input: {} } });
    sse(res, 'content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify({ command: 'echo hello > fake-out.txt', description: 'Write a file' }) } });
    sse(res, 'content_block_stop', { type: 'content_block_stop', index: 0 });
    sse(res, 'message_delta', { type: 'message_delta', delta: { stop_reason: 'tool_use', stop_sequence: null }, usage: { output_tokens: 20 } });
  } else {
    sse(res, 'content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } });
    await sleep(300);
    sse(res, 'content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: 'All done. I wrote the file.' } });
    sse(res, 'content_block_stop', { type: 'content_block_stop', index: 0 });
    sse(res, 'message_delta', { type: 'message_delta', delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: 8 } });
  }
  sse(res, 'message_stop', { type: 'message_stop' });
  res.end();
});
server.listen(port, '127.0.0.1', () => console.log('fake api on', port));
