import { describe, expect, it } from 'vitest';
import { muxMp4, type Sample } from './mp4Mux';

/* ---------- a small box walker used to read the output back ---------- */

interface BoxInfo {
  type: string;
  start: number;
  payload: number;
  end: number;
}

const CONTAINERS = ['moov', 'trak', 'mdia', 'minf', 'stbl', 'dinf'];

function readType(bytes: Uint8Array, at: number): string {
  return String.fromCharCode(...bytes.subarray(at + 4, at + 8));
}

function readBoxes(bytes: Uint8Array, start: number, end: number): BoxInfo[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const out: BoxInfo[] = [];
  let at = start;
  while (at < end) {
    const size = view.getUint32(at);
    if (size < 8 || at + size > end) throw new Error(`bad box at ${at}`);
    out.push({ type: readType(bytes, at), start: at, payload: at + 8, end: at + size });
    at += size;
  }
  return out;
}

function child(bytes: Uint8Array, box: BoxInfo, type: string, skip = 0): BoxInfo {
  const found = readBoxes(bytes, box.payload + skip, box.end).find((b) => b.type === type);
  if (!found) throw new Error(`no ${type} in ${box.type}`);
  return found;
}

function view(bytes: Uint8Array, box: BoxInfo): DataView {
  return new DataView(bytes.buffer, bytes.byteOffset + box.payload, box.end - box.payload);
}

/** The stbl box, found by walking moov > trak > mdia > minf > stbl. */
function stbl(bytes: Uint8Array): BoxInfo {
  const moov = readBoxes(bytes, 0, bytes.length).find((b) => b.type === 'moov');
  if (!moov) throw new Error('no moov');
  const trak = child(bytes, moov, 'trak');
  return child(bytes, child(bytes, child(bytes, trak, 'mdia'), 'minf'), 'stbl');
}

/** Every box type in the file, walking the containers we write. */
function allTypes(bytes: Uint8Array, boxes: BoxInfo[]): string[] {
  const out: string[] = [];
  for (const box of boxes) {
    out.push(box.type);
    if (CONTAINERS.includes(box.type)) {
      allTypes(bytes, readBoxes(bytes, box.payload, box.end)).forEach((t) => out.push(t));
    }
    if (box.type === 'stsd') {
      // A sample entry starts with 78 fixed bytes, then its child boxes.
      const avc1 = child(bytes, box, 'avc1', 8);
      out.push(avc1.type);
      allTypes(bytes, readBoxes(bytes, avc1.payload + 78, avc1.end)).forEach((t) => out.push(t));
    }
  }
  return out;
}

/* ---------- fixtures ---------- */

const AVC_RECORD = Uint8Array.of(1, 0x64, 0x00, 0x28, 0xff, 0xe1, 0x00, 0x04, 0x67, 0x64, 0x00, 0x28);

function sample(bytes: number[], durationTicks: number, key: boolean): Sample {
  return { data: Uint8Array.from(bytes), durationTicks, key };
}

function sizeOf(bytes: Uint8Array, at: number): number {
  return new DataView(bytes.buffer, bytes.byteOffset).getUint32(at);
}

describe('muxMp4 box layout', () => {
  it('starts with an ftyp box of size 32 with the brands', () => {
    const out = muxMp4({
      width: 1280,
      height: 720,
      timescale: 30000,
      avcC: AVC_RECORD,
      samples: [sample([1, 2, 3], 1000, true)],
    });
    expect(sizeOf(out, 0)).toBe(32);
    expect(readType(out, 0)).toBe('ftyp');
    // Major brand at 8, minor version at 12, then four compatible brands.
    expect(String.fromCharCode(...out.subarray(8, 12))).toBe('isom');
    expect(String.fromCharCode(...out.subarray(16, 32))).toBe('isomiso2avc1mp41');
  });

  it('puts moov before mdat, and the top-level boxes add up to the file size', () => {
    const out = muxMp4({
      width: 320,
      height: 240,
      timescale: 1000,
      avcC: AVC_RECORD,
      samples: [sample([9, 9], 500, true), sample([8, 8, 8], 500, false)],
    });
    const top = readBoxes(out, 0, out.length);
    expect(top.map((b) => b.type)).toEqual(['ftyp', 'moov', 'mdat']);
    expect(top[2].end).toBe(out.length);
    expect(top[2].end - top[2].payload).toBe(5); // 2 + 3 sample bytes
  });

  it('writes no udta, meta or other user-data atoms and zero creation/modification times', () => {
    const out = muxMp4({
      width: 1280,
      height: 720,
      timescale: 30000,
      avcC: AVC_RECORD,
      samples: [sample([1, 2], 1001, true), sample([3, 4], 1001, false)],
    });
    const types = allTypes(out, readBoxes(out, 0, out.length));
    expect(types).not.toContain('udta');
    expect(types).not.toContain('meta');
    expect(types).not.toContain('©nam');
    const moov = readBoxes(out, 0, out.length).find((b) => b.type === 'moov')!;
    const mvhd = child(out, moov, 'mvhd');
    const mv = view(out, mvhd);
    expect(mv.getUint32(4)).toBe(0); // creation time
    expect(mv.getUint32(8)).toBe(0); // modification time
    const trak = child(out, moov, 'trak');
    const tv = view(out, child(out, trak, 'tkhd'));
    expect(tv.getUint32(4)).toBe(0);
    expect(tv.getUint32(8)).toBe(0);
  });
});

describe('muxMp4 sample tables', () => {
  it('lists one size and one offset per sample, and each offset holds that sample', () => {
    const samples = [
      sample([0x11, 0x12, 0x13, 0x14], 1000, true),
      sample([0x21, 0x22], 1000, false),
      sample([0x31, 0x32, 0x33, 0x34, 0x35], 1000, false),
    ];
    const out = muxMp4({ width: 64, height: 48, timescale: 1000, avcC: AVC_RECORD, samples });
    const table = stbl(out);

    const stsz = view(out, child(out, table, 'stsz'));
    expect(stsz.getUint32(4)).toBe(0); // sizes are per sample
    expect(stsz.getUint32(8)).toBe(3);
    expect([stsz.getUint32(12), stsz.getUint32(16), stsz.getUint32(20)]).toEqual([4, 2, 5]);

    const stco = view(out, child(out, table, 'stco'));
    expect(stco.getUint32(4)).toBe(3);
    samples.forEach((s, i) => {
      const at = stco.getUint32(8 + i * 4);
      expect(Array.from(out.subarray(at, at + s.data.length))).toEqual(Array.from(s.data));
    });

    // The first sample starts right after the mdat header, which follows moov.
    const mdat = readBoxes(out, 0, out.length).find((b) => b.type === 'mdat')!;
    expect(stco.getUint32(8)).toBe(mdat.payload);
  });

  it('lists the key frames in stss as 1-based sample numbers', () => {
    const keys = [true, false, false, true, false];
    const samples = keys.map((key, i) => sample([i + 1, i + 1], 1000, key));
    const out = muxMp4({ width: 64, height: 48, timescale: 1000, avcC: AVC_RECORD, samples });
    const stss = view(out, child(out, stbl(out), 'stss'));
    expect(stss.getUint32(4)).toBe(2);
    expect([stss.getUint32(8), stss.getUint32(12)]).toEqual([1, 4]);
  });

  it('run-length encodes the durations in stts', () => {
    const samples = [
      sample([1], 3000, true),
      sample([2], 3000, false),
      sample([3], 3000, false),
      sample([4], 1500, false),
    ];
    const out = muxMp4({ width: 64, height: 48, timescale: 30000, avcC: AVC_RECORD, samples });
    const stts = view(out, child(out, stbl(out), 'stts'));
    expect(stts.getUint32(4)).toBe(2);
    expect([stts.getUint32(8), stts.getUint32(12)]).toEqual([3, 3000]);
    expect([stts.getUint32(16), stts.getUint32(20)]).toEqual([1, 1500]);
  });

  it('writes the timescale and total duration in mdhd', () => {
    const samples = [sample([1], 3000, true), sample([2], 3000, false)];
    const out = muxMp4({ width: 64, height: 48, timescale: 30000, avcC: AVC_RECORD, samples });
    const moov = readBoxes(out, 0, out.length).find((b) => b.type === 'moov')!;
    const mdhd = view(out, child(out, child(out, child(out, moov, 'trak'), 'mdia'), 'mdhd'));
    expect(mdhd.getUint32(12)).toBe(30000);
    expect(mdhd.getUint32(16)).toBe(6000);
  });

  it('lists one chunk per sample in stsc', () => {
    const samples = [sample([1], 100, true), sample([2], 100, false)];
    const out = muxMp4({ width: 64, height: 48, timescale: 1000, avcC: AVC_RECORD, samples });
    const stsc = view(out, child(out, stbl(out), 'stsc'));
    expect(stsc.getUint32(4)).toBe(1);
    expect([stsc.getUint32(8), stsc.getUint32(12), stsc.getUint32(16)]).toEqual([1, 1, 1]);
  });
});

describe('muxMp4 sample entry', () => {
  it('puts the avcC record inside avc1 with the box header added', () => {
    const out = muxMp4({
      width: 1280,
      height: 720,
      timescale: 30000,
      avcC: AVC_RECORD,
      samples: [sample([1, 2, 3], 1000, true)],
    });
    const stsd = child(out, stbl(out), 'stsd');
    const avc1 = child(out, stsd, 'avc1', 8);
    const avcC = child(out, avc1, 'avcC', 78);
    expect(Array.from(out.subarray(avcC.payload, avcC.end))).toEqual(Array.from(AVC_RECORD));
    // Width and height sit at 24 and 26 inside the 78-byte fixed part.
    expect(new DataView(out.buffer, out.byteOffset + avc1.payload).getUint16(24)).toBe(1280);
    expect(new DataView(out.buffer, out.byteOffset + avc1.payload).getUint16(26)).toBe(720);
  });

  it('does not wrap a complete avcC box a second time', () => {
    const boxed = Uint8Array.of(0, 0, 0, 12, 0x61, 0x76, 0x63, 0x43, ...AVC_RECORD.subarray(0, 4));
    const out = muxMp4({
      width: 64,
      height: 48,
      timescale: 1000,
      avcC: boxed,
      samples: [sample([1], 100, true)],
    });
    const avc1 = child(out, child(out, stbl(out), 'stsd'), 'avc1', 8);
    const avcC = child(out, avc1, 'avcC', 78);
    expect(avcC.end - avcC.start).toBe(12);
    expect(readBoxes(out, avc1.payload + 78, avc1.end)).toHaveLength(1);
  });
});

describe('muxMp4 edge cases', () => {
  it('handles zero samples', () => {
    const out = muxMp4({ width: 64, height: 48, timescale: 1000, avcC: AVC_RECORD, samples: [] });
    const top = readBoxes(out, 0, out.length);
    expect(top.map((b) => b.type)).toEqual(['ftyp', 'moov', 'mdat']);
    expect(top[2].end - top[2].start).toBe(8);
    expect(view(out, child(out, stbl(out), 'stco')).getUint32(4)).toBe(0);
    expect(view(out, child(out, stbl(out), 'stsz')).getUint32(8)).toBe(0);
  });

  it('handles one key sample', () => {
    const out = muxMp4({
      width: 64,
      height: 48,
      timescale: 1000,
      avcC: AVC_RECORD,
      samples: [sample([7, 7, 7], 1000, true)],
    });
    const stco = view(out, child(out, stbl(out), 'stco'));
    expect(stco.getUint32(4)).toBe(1);
    const at = stco.getUint32(8);
    expect(Array.from(out.subarray(at, at + 3))).toEqual([7, 7, 7]);
    expect(out.length).toBe(at + 3);
  });

  it('keeps offsets right for 840 frames (28 s at 30 fps)', () => {
    const samples: Sample[] = [];
    for (let i = 0; i < 840; i++) {
      // 1000 ticks at 30000 per second is 1/30 s, so 840 frames is 28 s.
      samples.push(sample(Array.from({ length: 5 + (i % 7) }, () => i & 0xff), 1000, i % 60 === 0));
    }
    const out = muxMp4({ width: 1280, height: 720, timescale: 30000, avcC: AVC_RECORD, samples });
    const table = stbl(out);
    const stco = view(out, child(out, table, 'stco'));
    const stsz = view(out, child(out, table, 'stsz'));
    expect(stco.getUint32(4)).toBe(840);
    expect(stsz.getUint32(8)).toBe(840);
    for (const i of [0, 1, 419, 839]) {
      const at = stco.getUint32(8 + i * 4);
      expect(out[at]).toBe(i & 0xff);
      expect(stsz.getUint32(12 + i * 4)).toBe(5 + (i % 7));
    }
    const mvhd = child(out, readBoxes(out, 0, out.length).find((b) => b.type === 'moov')!, 'mvhd');
    expect(view(out, mvhd).getUint32(16)).toBe(28000);
  });

  it('rejects a first sample that is not a key frame', () => {
    expect(() =>
      muxMp4({ width: 64, height: 48, timescale: 1000, avcC: AVC_RECORD, samples: [sample([1], 1, false)] }),
    ).toThrow(/key frame/);
  });

  it('rejects a bad size, timescale or duration', () => {
    const one = [sample([1], 1, true)];
    expect(() => muxMp4({ width: 0, height: 48, timescale: 1000, avcC: AVC_RECORD, samples: one })).toThrow();
    expect(() => muxMp4({ width: 64, height: 70000, timescale: 1000, avcC: AVC_RECORD, samples: one })).toThrow();
    expect(() => muxMp4({ width: 64, height: 48, timescale: 0, avcC: AVC_RECORD, samples: one })).toThrow();
    expect(() =>
      muxMp4({ width: 64, height: 48, timescale: 1000, avcC: AVC_RECORD, samples: [sample([1], -1, true)] }),
    ).toThrow();
  });
});
