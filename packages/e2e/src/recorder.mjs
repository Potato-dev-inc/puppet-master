// Screencast over CDP -> JPEG frames -> one small H.264 mp4 (ffmpeg). Frames go to a folder next to the video and are
// deleted file by file once the video is written.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

export function ffmpegPath() {
  if (process.env.FFMPEG) return process.env.FFMPEG;
  const probe = spawnSync(process.platform === 'win32' ? 'where' : 'which', ['ffmpeg'], { encoding: 'utf8' });
  if (probe.status === 0) return probe.stdout.split(/\r?\n/)[0].trim();
  for (const p of ['C:/ffmpeg/bin/ffmpeg.exe', '/usr/bin/ffmpeg', '/opt/homebrew/bin/ffmpeg']) if (fs.existsSync(p)) return p;
  return null;
}

export class Recorder {
  /** @param conn CDP connection @param {string} dir where the frames and the video go */
  constructor(conn, dir, { width, height } = {}) {
    this.c = conn;
    this.dir = dir;
    this.framesDir = path.join(dir, 'frames');
    this.frames = [];
    this.width = width;
    this.height = height;
    this.startedAt = 0;
    this.running = false;
  }

  async start() {
    fs.mkdirSync(this.framesDir, { recursive: true });
    this.handler = (m) => {
      if (m.method !== 'Page.screencastFrame') return;
      const { data, metadata, sessionId } = m.params;
      const name = `f${String(this.frames.length + 1).padStart(6, '0')}.jpg`;
      try { fs.writeFileSync(path.join(this.framesDir, name), Buffer.from(data, 'base64')); this.frames.push({ name, t: metadata.timestamp * 1000 }); } catch { /* disk full: drop the frame */ }
      this.c.send('Page.screencastFrameAck', { sessionId }).catch(() => {});
    };
    this.off = this.c.on(this.handler);
    this.startedAt = Date.now();
    await this.c.send('Page.startScreencast', { format: 'jpeg', quality: 72, maxWidth: this.width || 1280, maxHeight: this.height || 800, everyNthFrame: 1 });
    this.running = true;
  }

  async stop() {
    if (!this.running) return;
    this.running = false;
    try { await this.c.send('Page.stopScreencast'); } catch { /* the page is gone */ }
    if (this.off) this.off();
  }

  /** Write the mp4. Returns { file, seconds } or null when there was nothing to write. */
  encode(outFile) {
    const ffmpeg = ffmpegPath();
    if (!ffmpeg) throw new Error('ffmpeg not found (put it on PATH or set FFMPEG)');
    if (!this.frames.length) return null;
    const lines = [];
    let total = 0;
    for (let i = 0; i < this.frames.length; i++) {
      const f = this.frames[i];
      const next = this.frames[i + 1];
      const dur = Math.min(4, Math.max(0.001, next ? (next.t - f.t) / 1000 : 0.8));
      total += dur;
      lines.push(`file '${f.name}'`, `duration ${dur.toFixed(3)}`);
    }
    lines.push(`file '${this.frames[this.frames.length - 1].name}'`);
    const list = path.join(this.framesDir, 'list.txt');
    fs.writeFileSync(list, lines.join('\n') + '\n');
    const W = (this.width || 1280) & ~1;
    const H = (this.height || 800) & ~1;
    const args = ['-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', 'list.txt',
      '-vf', `fps=25,scale=${W}:${H}:force_original_aspect_ratio=decrease,pad=${W}:${H}:(ow-iw)/2:(oh-ih)/2:color=black,format=yuv420p`, '-c:v', 'libx264', '-preset', 'veryfast', '-crf', process.env.E2E_CRF || '28',
      '-movflags', '+faststart', path.resolve(outFile)];
    const r = spawnSync(ffmpeg, args, { cwd: this.framesDir, encoding: 'utf8' });
    if (r.status !== 0) throw new Error('ffmpeg failed: ' + (r.stderr || '').slice(0, 400));
    if (process.env.E2E_DEBUG) console.log(`recorder: ${this.frames.length} frames, ${total.toFixed(1)} s of frame time, first->last ${((this.frames[this.frames.length - 1].t - this.frames[0].t) / 1000).toFixed(1)} s`);
    return { file: outFile, seconds: total };
  }

  /** Remove the frame files (one by one) and the folder. */
  cleanup() {
    try {
      for (const f of fs.readdirSync(this.framesDir)) fs.unlinkSync(path.join(this.framesDir, f));
      fs.rmdirSync(this.framesDir);
    } catch { /* leave what is left */ }
  }

  /** Save one frame as a still (for the dashboard thumbnail and for looking at the video quickly). */
  keepFrame(index, outFile) {
    const f = this.frames[Math.max(0, Math.min(index, this.frames.length - 1))];
    if (f) fs.copyFileSync(path.join(this.framesDir, f.name), outFile);
  }
}
