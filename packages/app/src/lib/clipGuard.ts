// Second wall for Clip It (SPEC-n32 section 3.5 step 7). Rust (clip/scrub.rs) does the real
// redaction; this only catches a future Rust bug before a frame is drawn. Keep the shapes in step with scrub.rs.

const HIDDEN = '[hidden]';

const SHAPES: RegExp[] = [
  /\bsk-[A-Za-z0-9_-]{12,}/g, // API keys (sk-...)
  /\b(?:ghp|github_pat)_[A-Za-z0-9_]{10,}/g, // GitHub tokens
  /\bAKIA[0-9A-Z]{12,}/g, // AWS access key ids
  /\bxox[a-z]-[A-Za-z0-9-]{6,}/g, // Slack tokens
  /\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g, // JWT (starts with eyJ)
  /-----BEGIN[\s\S]*/g, // private key block: hide everything after it
  /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, // e-mail address
  /[A-Za-z]:[\\/]+Users[\\/]\S*/gi, // Windows home folder
  /\/(?:home|Users)\/\S*/g, // Linux and macOS home folder
  /[A-Za-z0-9]{30,}/g, // long opaque run
];

/** Hides anything that still looks private. `hit` is true when something was hidden. */
export function guardText(text: string): { text: string; hit: boolean } {
  let out = text;
  for (const re of SHAPES) out = out.replace(re, HIDDEN);
  return { text: out, hit: out !== text };
}

/** Guards many strings and counts how many of them had something hidden. */
export class GuardCounter {
  private count = 0;

  guard(text: string): string {
    const r = guardText(text);
    if (r.hit) this.count += 1;
    return r.text;
  }

  get hits(): number {
    return this.count;
  }
}
