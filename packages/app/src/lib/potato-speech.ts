// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7,
// file packages/core/src/speech-text.ts. Apache-2.0. Changed by Puppet Master: the MessageBlock type and
// speechFromBlocks are dropped (our thread holds plain text), the spoken yes/no helpers are dropped (a voice never
// gives consent here), table rows stop at their own line break (the original fused two rows into one word),
// toUtterances is kept as the splitter for speechSynthesis, and the browser voice layer below (voices, the
// queue, Stop) is ours.
//
// Potato Bot's "read aloud": the webview's own speech (`speechSynthesis`, the Windows voices). No key, and the words
// never leave the PC. Pure helpers first, then the small browser layer, which tests replace with a fake.

import type { PotatoVoice } from './potato';

// ---- what to say: markdown and agent output -> a line a voice can read ----------------------------------------------

/** A fenced block becomes a mention of itself, with its language if known. */
function describeCodeBlock(fence: string): string {
  const lang =
    fence
      .trim()
      .split(/\s+/)[0]
      ?.replace(/[^a-z0-9+#]/gi, "") ?? "";
  const spoken: Record<string, string> = {
    ts: "TypeScript",
    tsx: "TypeScript",
    js: "JavaScript",
    jsx: "JavaScript",
    py: "Python",
    sh: "shell",
    bash: "shell",
    zsh: "shell",
    json: "JSON",
    yml: "YAML",
    yaml: "YAML",
    sql: "SQL",
    rs: "Rust",
    go: "Go",
    swift: "Swift",
    diff: "diff",
  };
  const name = spoken[lang.toLowerCase()];
  return name ? `. (a ${name} code block) ` : ". (a code block) ";
}

/** `a/b/c/file.ts` → `file.ts`. A path's directories are for the eye. */
function shortenPaths(text: string): string {
  return text.replace(/(?:[\w.@-]+\/){1,}([\w.-]+\.\w{1,6})\b/g, "$1");
}

const ENDS_SENTENCE = /[.!?:;]\s*$/;

/**
 * Markdown (or any agent output) → a line a voice can read.
 *
 * Say the prose, name the artifacts, drop the syntax. Pure and synchronous
 * so it stays easy to tune against real transcripts.
 */
export function speakable(input: string): string {
  if (!input) return "";
  let text = input;

  text = text.replace(/```([^\n]*)\n[\s\S]*?(?:```|$)/g, (_m, fence: string) =>
    describeCodeBlock(fence),
  );
  text = text.replace(/~~~([^\n]*)\n[\s\S]*?(?:~~~|$)/g, (_m, fence: string) =>
    describeCodeBlock(fence),
  );

  text = text.replace(/!\[([^\]]*)\]\([^)]*\)/g, (_m, alt: string) =>
    alt ? `. (image: ${alt}) ` : ". (an image) ",
  );
  text = text.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1");
  text = text.replace(/<https?:\/\/[^>\s]+>/g, " a link ");
  text = text.replace(/\bhttps?:\/\/\S+/g, " a link ");

  // Table rows end at their own line: `\s*` here would swallow the line break and fuse two rows into one word.
  text = text.replace(/^[ \t]*\|?[ \t:-]*\|[ \t|:-]*$/gm, "");
  text = text.replace(/^[ \t]*\|(.+)\|[ \t]*$/gm, (_m, row: string) =>
    row
      .split("|")
      .map((cell) => cell.trim())
      .filter(Boolean)
      .join(", "),
  );

  text = text.replace(/`([^`\n]+)`/g, (_m, code: string) =>
    code.length <= 40 ? code : " that snippet ",
  );

  text = text.replace(/^\s{0,3}#{1,6}\s+(.*)$/gm, (_m, head: string) =>
    ENDS_SENTENCE.test(head) ? head : `${head.trim()}.`,
  );

  text = text.replace(/^\s*[-*+]\s+/gm, "");
  text = text.replace(/^\s*\d+[.)]\s+/gm, "");
  text = text.replace(/^\s*>\s?/gm, "");
  text = text.replace(/^\s*(?:[-*_]\s*){3,}$/gm, "");

  text = text.replace(/(\*\*|__)(.*?)\1/g, "$2");
  text = text.replace(/(\*|_)(?=\S)(.*?)(?<=\S)\1/g, "$2");
  text = text.replace(/~~(.*?)~~/g, "$1");

  text = text.replace(/\[[ xX]\]\s*/g, "");

  text = shortenPaths(text);

  text = text.replace(
    /[\u{1F000}-\u{1FAFF}]|[\u{2600}-\u{27BF}]|[\u{FE00}-\u{FE0F}]|[\u{2190}-\u{21FF}\u{2B00}-\u{2BFF}]/gu,
    "",
  );

  text = text.replace(/\n{2,}/g, ". ");
  text = text.replace(/\n/g, ". ");

  text = text.replace(/\s+/g, " ");
  text = text.replace(/\s+([.,!?;:])/g, "$1");
  text = text.replace(/(?:\.\s*){2,}/g, ". ");
  text = text.replace(/,\s*\./g, ".");
  text = text.trim();

  return /[\p{L}\p{N}]/u.test(text) ? text : "";
}

const BOUNDARY =
  /(?<!\b(?:e\.g|i\.e|etc|vs|Dr|Mr|Mrs|Ms|No|approx))(?<![.\d])([.!?])(["')\]]*)\s+/g;

/**
 * Split speakable text into utterances a synthesizer can start on.
 * Short fragments are glued onto their neighbour so the voice keeps context.
 * maxChars bounds UTF-16 code units, matching the synthesis API's limit.
 */
export function toUtterances(input: string, { minChars = 12, maxChars = 320 } = {}): string[] {
  if (!Number.isInteger(maxChars) || maxChars < 2) {
    throw new RangeError("maxChars must be an integer of at least 2");
  }
  const text = speakable(input);
  if (!text) return [];

  const MARK = "\u0000";
  const rough = text
    .replace(BOUNDARY, `$1$2${MARK}`)
    .split(MARK)
    .map((s) => s.trim())
    .filter(Boolean);

  const out: string[] = [];
  for (const piece of rough) {
    const parts = piece.length <= maxChars ? [piece] : splitLong(piece, maxChars);
    for (const [index, part] of parts.entries()) {
      const prev = out[out.length - 1];
      if (
        index === 0 &&
        prev &&
        (prev.length < minChars || part.length < minChars) &&
        prev.length + 1 + part.length <= maxChars
      ) {
        out[out.length - 1] = `${prev} ${part}`;
      } else {
        out.push(part);
      }
    }
  }
  return out;
}

function splitLong(text: string, maxChars: number): string[] {
  const out: string[] = [];
  const segmenter =
    typeof Intl.Segmenter === "function"
      ? new Intl.Segmenter(undefined, { granularity: "grapheme" })
      : undefined;
  let rest = text;
  while (rest.length > maxChars) {
    const window = rest.slice(0, maxChars);
    const at = Math.max(
      window.lastIndexOf(", "),
      window.lastIndexOf("; "),
      window.lastIndexOf(" — "),
    );
    let cut = at > maxChars / 2 ? at + 1 : window.lastIndexOf(" ");
    if (cut <= 0) {
      cut = 0;
      if (segmenter) {
        for (const { index, segment } of segmenter.segment(rest)) {
          const end = index + segment.length;
          if (end > maxChars) break;
          cut = end;
        }
      }
      // A single grapheme may exceed the budget; older native runtimes may
      // also lack Segmenter. Still make progress without splitting a surrogate pair.
      if (cut === 0) {
        cut = maxChars;
        const last = rest.charCodeAt(cut - 1);
        if (last >= 0xd800 && last <= 0xdbff) cut -= 1;
      }
    }
    out.push(rest.slice(0, cut).trim());
    rest = rest.slice(cut).trim();
  }
  if (rest) out.push(rest);
  return out;
}

// ---- voices and settings ----------------------------------------------------------------------------------------------

export const RATE_MIN = 50;
export const RATE_MAX = 200;
export const DEFAULT_RATE = 100;
/** What "Test voice" says. Short and plain. */
export const SAMPLE_TEXT = "Hi, this is how I'll sound when I read replies out loud.";

/** A speed in percent, kept between 50 and 200 and to whole numbers. A value that is not a number is normal speed. */
export function clampRate(percent: number): number {
  if (!Number.isFinite(percent)) return DEFAULT_RATE;
  return Math.min(RATE_MAX, Math.max(RATE_MIN, Math.round(percent)));
}

/** The saved voice settings with every missing field filled in. */
export function normalizeVoice(voice?: Partial<PotatoVoice> | null): PotatoVoice {
  return {
    voiceUri: voice?.voiceUri ?? "",
    ratePercent: clampRate(voice?.ratePercent ?? DEFAULT_RATE),
    readAloudBots: [...(voice?.readAloudBots ?? [])],
  };
}

/** The part of a browser voice this file reads. */
export interface SpeechVoiceLike {
  voiceURI: string;
  name: string;
  lang: string;
}

/** The part of a browser utterance this file sets. */
export interface SpeechUtteranceLike {
  text: string;
  voice: SpeechVoiceLike | null;
  rate: number;
  onend: (() => void) | null;
  onerror: (() => void) | null;
}

/** The part of `speechSynthesis` this file uses. Tests pass a fake; the webview passes the real one. */
export interface SpeechSynthesisLike {
  getVoices(): SpeechVoiceLike[];
  speak(utterance: SpeechUtteranceLike): void;
  cancel(): void;
  addEventListener?(type: "voiceschanged", listener: () => void): void;
  removeEventListener?(type: "voiceschanged", listener: () => void): void;
}

type UtteranceConstructor = new (text: string) => SpeechUtteranceLike;

function speechGlobals(): { synth: SpeechSynthesisLike; Utterance: UtteranceConstructor } | null {
  const g = globalThis as unknown as { speechSynthesis?: SpeechSynthesisLike; SpeechSynthesisUtterance?: UtteranceConstructor };
  if (!g.speechSynthesis || typeof g.SpeechSynthesisUtterance !== "function") return null;
  return { synth: g.speechSynthesis, Utterance: g.SpeechSynthesisUtterance };
}

/** Does this webview have speech at all? Without it the Speak button and the switch are not shown. */
export function speechSupported(): boolean {
  return speechGlobals() !== null;
}

export function listVoices(): SpeechVoiceLike[] {
  return speechGlobals()?.synth.getVoices() ?? [];
}

/** Calls `cb` when the voice list changes (Chrome and WebView2 fill it in late). Returns the way to stop listening. */
export function onVoicesChanged(cb: () => void): () => void {
  const synth = speechGlobals()?.synth;
  if (!synth?.addEventListener) return () => {};
  synth.addEventListener("voiceschanged", cb);
  return () => synth.removeEventListener?.("voiceschanged", cb);
}

/** The language part of a voice's tag: `en-US` becomes `en`. */
export function languageOf(voice: { lang: string }): string {
  return (voice.lang || "").split(/[-_]/)[0].toLowerCase();
}

/** Every language the voices speak, once each, A to Z. */
export function languagesOf(voices: readonly SpeechVoiceLike[]): string[] {
  return [...new Set(voices.map(languageOf).filter(Boolean))].sort();
}

/** The voices in one language (`en`, `fr`...). */
export function voicesInLanguage(voices: readonly SpeechVoiceLike[], language: string): SpeechVoiceLike[] {
  return voices.filter((v) => languageOf(v) === language.toLowerCase());
}

/** The app's own language as a short code (`en`), the default for the language filter. */
export function appLanguage(tag: string = typeof navigator !== "undefined" ? navigator.language : "en"): string {
  return (tag || "en").split(/[-_]/)[0].toLowerCase() || "en";
}

/** The voice with this URI, or null (the system's default) when it is empty or no longer installed. */
export function pickVoice(voices: readonly SpeechVoiceLike[], voiceUri: string): SpeechVoiceLike | null {
  if (!voiceUri) return null;
  return voices.find((v) => v.voiceURI === voiceUri) ?? null;
}

// ---- speaking: one queue, and a Stop that always works ------------------------------------------------------------------

export interface SpeakPrefs {
  voiceUri: string;
  ratePercent: number;
}

/** Bumped by every stop. A late `onend` from an older batch sees a new number and does nothing. */
let generation = 0;
/** Utterances queued and not yet ended. */
let pending = 0;
/** The message being read (the last one queued), or null when nothing is. */
let speakingId: string | null = null;
const listeners = new Set<() => void>();

function tell(): void {
  listeners.forEach((l) => l());
}

/** Calls `cb` whenever the message being read changes. Returns the way to stop listening. */
export function subscribeSpeaking(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** The id of the message being read, or null. */
export function speakingMessageId(): string | null {
  return speakingId;
}

/** Stops now: the queue is cleared and no message shows Stop any more. */
export function stopSpeaking(): void {
  generation += 1;
  pending = 0;
  speechGlobals()?.synth.cancel();
  if (speakingId !== null) {
    speakingId = null;
    tell();
  }
}

/**
 * Reads one reply aloud. `replace` stops whatever is being read first (the Speak button). Without it the reply
 * waits its turn in the queue (a new reply while the page is open). Returns false when there is nothing to say or
 * this webview cannot speak.
 */
export function speakReply(id: string, text: string, prefs: SpeakPrefs, replace: boolean): boolean {
  const g = speechGlobals();
  if (!g) return false;
  const chunks = toUtterances(text);
  if (chunks.length === 0) return false;
  if (replace) stopSpeaking();
  const mine = generation;
  const voice = pickVoice(g.synth.getVoices(), prefs.voiceUri);
  const rate = clampRate(prefs.ratePercent) / 100;
  const finish = () => {
    if (mine !== generation) return;
    pending = Math.max(0, pending - 1);
    if (pending === 0 && speakingId !== null) {
      speakingId = null;
      tell();
    }
  };
  speakingId = id;
  tell();
  for (const chunk of chunks) {
    const utterance = new g.Utterance(chunk);
    utterance.voice = voice;
    utterance.rate = rate;
    utterance.onend = finish;
    utterance.onerror = finish;
    pending += 1;
    g.synth.speak(utterance);
  }
  return true;
}
