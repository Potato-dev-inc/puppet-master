/**
 * A small MP4 writer for one H.264 video track (used by the Clip It feature).
 *
 * Layout: ftyp, moov, mdat. The moov box comes before the sample bytes so a
 * player can start at once. Every sample is its own chunk, so each stco entry
 * is the absolute file offset of one sample. No audio, no B-frames, no
 * udta/meta atoms, and creation/modification times are zero.
 *
 * Box sizes are 32-bit (big-endian). Full boxes carry version and flags.
 */

export interface Sample {
  /** One access unit in AVCC form (4-byte length prefixes), as WebCodecs gives it. */
  data: Uint8Array;
  /** Duration in timescale ticks. */
  durationTicks: number;
  /** True for an IDR (sync) frame. */
  key: boolean;
}

export interface Mp4Args {
  width: number;
  height: number;
  /** Ticks per second for the track (for example 30000 for 30 fps frames). */
  timescale: number;
  /**
   * The AVCDecoderConfigurationRecord (WebCodecs `decoderConfig.description`).
   * A complete avcC box (header included) is also accepted.
   */
  avcC: Uint8Array;
  samples: Sample[];
}

/** Movie timescale used by mvhd and tkhd. */
const MOVIE_TIMESCALE = 1000;
const MAX_U32 = 0xffffffff;

export function muxMp4(args: Mp4Args): Uint8Array {
  const { width, height, timescale, avcC, samples } = args;
  checkArgs(width, height, timescale, avcC, samples);

  const totalTicks = samples.reduce((sum, s) => sum + s.durationTicks, 0);
  if (totalTicks > MAX_U32) throw new Error('Clip is too long for one MP4 track.');
  const mdatPayload = samples.reduce((sum, s) => sum + s.data.length, 0);
  if (mdatPayload + 8 > MAX_U32) throw new Error('Clip is too large for one MP4 file.');

  const ftyp = buildFtyp();
  // The moov size does not depend on the offset values, so measure it first.
  const moovSize = buildMoov(args, totalTicks, 0).length;
  const firstSample = ftyp.length + moovSize + 8;
  const moov = buildMoov(args, totalTicks, firstSample);

  const mdat = box('mdat', ...samples.map((s) => s.data));
  return concat([ftyp, moov, mdat]);
}

function checkArgs(
  width: number,
  height: number,
  timescale: number,
  avcC: Uint8Array,
  samples: Sample[],
): void {
  if (!isU16(width) || width === 0) throw new Error('Video width must be 1 to 65535.');
  if (!isU16(height) || height === 0) throw new Error('Video height must be 1 to 65535.');
  if (!Number.isInteger(timescale) || timescale <= 0 || timescale > MAX_U32) {
    throw new Error('Timescale must be a positive whole number.');
  }
  if (avcC.length < 7) throw new Error('The H.264 config record is missing.');
  samples.forEach((s, i) => {
    if (!Number.isInteger(s.durationTicks) || s.durationTicks < 0 || s.durationTicks > MAX_U32) {
      throw new Error(`Sample ${i} has a bad duration.`);
    }
  });
  if (samples.length > 0 && !samples[0].key) {
    throw new Error('The first sample must be a key frame.');
  }
}

function buildFtyp(): Uint8Array {
  return box(
    'ftyp',
    ascii('isom'),
    u32(0),
    ascii('isom'),
    ascii('iso2'),
    ascii('avc1'),
    ascii('mp41'),
  );
}

/** Builds the moov box. `firstSample` is the file offset of the first sample byte. */
function buildMoov(args: Mp4Args, totalTicks: number, firstSample: number): Uint8Array {
  const { width, height, timescale, samples } = args;
  const movieDuration = Math.round((totalTicks * MOVIE_TIMESCALE) / timescale);
  if (movieDuration > MAX_U32) throw new Error('Clip is too long for one MP4 track.');

  const mvhd = full(
    'mvhd',
    0,
    0,
    u32(0), // creation time (zero on purpose)
    u32(0), // modification time (zero on purpose)
    u32(MOVIE_TIMESCALE),
    u32(movieDuration),
    u32(0x00010000), // rate 1.0
    u16(0x0100), // volume 1.0
    u16(0),
    u32(0),
    u32(0),
    MATRIX,
    zeros(24), // pre_defined
    u32(2), // next track id
  );

  const tkhd = full(
    'tkhd',
    0,
    3, // enabled and in movie
    u32(0),
    u32(0),
    u32(1), // track id
    u32(0),
    u32(movieDuration),
    zeros(8),
    u16(0), // layer
    u16(0), // alternate group
    u16(0), // volume (video track)
    u16(0),
    MATRIX,
    u32(width << 16),
    u32(height << 16),
  );

  const mdhd = full(
    'mdhd',
    0,
    0,
    u32(0),
    u32(0),
    u32(timescale),
    u32(totalTicks),
    u16(0x55c4), // language "und"
    u16(0),
  );

  const hdlr = full('hdlr', 0, 0, u32(0), ascii('vide'), zeros(12), u8(0)); // empty name

  const vmhd = full('vmhd', 0, 1, u16(0), zeros(6));
  const dinf = box('dinf', full('dref', 0, 0, u32(1), full('url ', 0, 1)));

  const stbl = box(
    'stbl',
    buildStsd(args),
    buildStts(samples),
    buildStss(samples),
    buildStsc(samples.length),
    buildStsz(samples),
    buildStco(samples, firstSample),
  );

  const minf = box('minf', vmhd, dinf, stbl);
  const mdia = box('mdia', mdhd, hdlr, minf);
  const trak = box('trak', tkhd, mdia);
  return box('moov', mvhd, trak);
}

function buildStsd(args: Mp4Args): Uint8Array {
  // Visual sample entry: 78 bytes of fixed fields, then child boxes (avcC).
  const entry = new Uint8Array(78);
  const view = new DataView(entry.buffer);
  view.setUint16(6, 1); // data_reference_index
  view.setUint16(24, args.width);
  view.setUint16(26, args.height);
  view.setUint32(28, 0x00480000); // 72 dpi horizontal
  view.setUint32(32, 0x00480000); // 72 dpi vertical
  view.setUint16(40, 1); // frame_count
  view.setUint16(74, 0x0018); // depth
  view.setInt16(76, -1); // pre_defined

  const avcCBox = hasBoxHeader(args.avcC, 'avcC') ? args.avcC : box('avcC', args.avcC);
  const avc1 = box('avc1', entry, avcCBox);
  return full('stsd', 0, 0, u32(1), avc1);
}

/** Run-length encodes the sample durations. */
function buildStts(samples: Sample[]): Uint8Array {
  const runs: Array<[number, number]> = [];
  for (const s of samples) {
    const last = runs[runs.length - 1];
    if (last && last[1] === s.durationTicks) last[0] += 1;
    else runs.push([1, s.durationTicks]);
  }
  return full('stts', 0, 0, u32(runs.length), ...runs.flatMap(([count, delta]) => [u32(count), u32(delta)]));
}

/** 1-based numbers of the key frames. */
function buildStss(samples: Sample[]): Uint8Array {
  const keys: number[] = [];
  samples.forEach((s, i) => {
    if (s.key) keys.push(i + 1);
  });
  return full('stss', 0, 0, u32(keys.length), ...keys.map((n) => u32(n)));
}

/** One chunk per sample: a single entry maps every chunk to one sample. */
function buildStsc(count: number): Uint8Array {
  if (count === 0) return full('stsc', 0, 0, u32(0));
  return full('stsc', 0, 0, u32(1), u32(1), u32(1), u32(1));
}

function buildStsz(samples: Sample[]): Uint8Array {
  return full(
    'stsz',
    0,
    0,
    u32(0), // 0 = each sample has its own size
    u32(samples.length),
    ...samples.map((s) => u32(s.data.length)),
  );
}

function buildStco(samples: Sample[], firstSample: number): Uint8Array {
  const offsets: Uint8Array[] = [];
  let at = firstSample;
  for (const s of samples) {
    offsets.push(u32(at));
    at += s.data.length;
  }
  return full('stco', 0, 0, u32(samples.length), ...offsets);
}

/* ---------- box helpers ---------- */

const MATRIX = concat([
  u32(0x00010000), u32(0), u32(0),
  u32(0), u32(0x00010000), u32(0),
  u32(0), u32(0), u32(0x40000000),
]);

/** A box: 32-bit size, 4-char type, then the payload parts. */
function box(type: string, ...payload: Uint8Array[]): Uint8Array {
  const body = concat(payload);
  const size = 8 + body.length;
  if (size > MAX_U32) throw new Error(`Box ${type} is too large.`);
  return concat([u32(size), ascii(type), body]);
}

/** A full box: the payload starts with version (1 byte) and flags (3 bytes). */
function full(type: string, version: number, flags: number, ...payload: Uint8Array[]): Uint8Array {
  return box(type, u32(((version << 24) | flags) >>> 0), ...payload);
}

function hasBoxHeader(bytes: Uint8Array, type: string): boolean {
  return bytes.length >= 8 && ascii(type).every((b, i) => bytes[4 + i] === b);
}

function concat(parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((sum, p) => sum + p.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

function u8(v: number): Uint8Array {
  return Uint8Array.of(v & 0xff);
}

function u16(v: number): Uint8Array {
  const out = new Uint8Array(2);
  new DataView(out.buffer).setUint16(0, v);
  return out;
}

function u32(v: number): Uint8Array {
  const out = new Uint8Array(4);
  new DataView(out.buffer).setUint32(0, v >>> 0);
  return out;
}

function zeros(n: number): Uint8Array {
  return new Uint8Array(n);
}

function ascii(text: string): Uint8Array {
  return Uint8Array.from(text, (c) => c.charCodeAt(0));
}

function isU16(v: number): boolean {
  return Number.isInteger(v) && v >= 0 && v <= 0xffff;
}
