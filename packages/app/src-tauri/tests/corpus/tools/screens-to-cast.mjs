#!/usr/bin/env node
// Turns a list of captured screens (with times) into an asciicast v2 file for the replay corpus.
//
//   node screens-to-cast.mjs spec.json out.cast
//
// A screen capture is plain text (what `read_terminal_buffer` or a fixture holds); each frame
// becomes "clear screen, home, the rows", the same bytes a TUI repaint would send, so the replay's
// screen model ends up showing exactly the captured rows.
//
// spec.json:
// {
//   "width": 120, "height": 30,
//   "title": "what this cast shows",
//   "pm": { "agent": "codex", "agent_version": "0.159.3", "bug": "one line",
//           "initial_status": "running" | "idle", "source": ["where the screens came from"] },
//   "events": [
//     { "t": 0.6,  "screen": "path/to/capture.txt" },     // or "text": "rows\nof\nthe screen"
//     { "t": 1.0,  "raw": "\u001b[31mbytes as written\u001b[0m" },
//     { "t": 5.0,  "resize": "141x51" },
//     { "t": 7.5,  "input": "\r" },                       // an input the app sent when it was recorded
//     { "t": 7.65, "marker": "edit:src/a.rs" },           // the agent changed a file
//     { "react": { "delay": 0.05, "on": "\u001b", "enter": false,
//                  "while_screen_contains": "esc skip", "screen": "path/to/after_esc.txt" } }
//   ]
// }
//
// Paths in a spec are relative to the spec file.
import fs from 'node:fs';
import path from 'node:path';

export function frame(text) {
  const rows = text.replace(/\r\n/g, '\n').replace(/\n+$/, '').split('\n');
  return '\u001b[2J\u001b[H' + rows.join('\r\n');
}

export function toCast(spec, baseDir = '.') {
  const read = (p) => fs.readFileSync(path.resolve(baseDir, p), 'utf8');
  const bytesOf = (e) => (e.screen ? frame(read(e.screen)) : e.text !== undefined ? frame(e.text) : e.raw);
  const header = {
    version: 2,
    width: spec.width,
    height: spec.height,
    timestamp: spec.timestamp ?? 0,
    title: spec.title,
    env: { TERM: 'xterm-256color' },
    pm: spec.pm,
  };
  const lines = [JSON.stringify(header)];
  const timeline = [];
  const reactions = [];
  for (const e of spec.events) {
    if (e.react) {
      const r = e.react;
      reactions.push([r.delay, 'ro', { on: r.on, enter: r.enter, while_screen_contains: r.while_screen_contains, data: bytesOf(r) }]);
    } else if (e.resize) timeline.push([e.t, 'r', e.resize]);
    else if (e.input !== undefined) timeline.push([e.t, 'i', e.input]);
    else if (e.marker !== undefined) timeline.push([e.t, 'm', e.marker]);
    else timeline.push([e.t, 'o', bytesOf(e)]);
  }
  timeline.sort((a, b) => a[0] - b[0]);
  for (const e of [...timeline, ...reactions]) lines.push(JSON.stringify(e));
  return lines.join('\n') + '\n';
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'))) {
  const [specPath, out] = process.argv.slice(2);
  if (!specPath || !out) {
    console.error('usage: node screens-to-cast.mjs spec.json out.cast');
    process.exit(2);
  }
  const spec = JSON.parse(fs.readFileSync(specPath, 'utf8'));
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, toCast(spec, path.dirname(path.resolve(specPath))));
  console.log(`wrote ${out}`);
}
