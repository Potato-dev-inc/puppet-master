import { isBackspaceEcho, isBackspaceKey } from './mirror-local-echo';

const BACKSPACE_ECHO_PATTERNS = ['\b \b', '\x08 \x08', '\b'];

function countPureBackspaceEchoes(chunk: string): number {
  let rest = chunk;
  let count = 0;

  while (rest.length > 0) {
    const pattern = BACKSPACE_ECHO_PATTERNS.find((candidate) => rest.startsWith(candidate));
    if (!pattern) return 0;
    rest = rest.slice(pattern.length);
    count += 1;
  }

  return count;
}

/** Tracks outbound keystrokes so mirror viewers can skip matching PTY echo. */
export class MirrorEchoFilter {
  private pending = '';
  private expectedBackspaceEchoes = 0;

  noteOutbound(text: string): void {
    for (const ch of text) {
      if (isBackspaceKey(ch)) {
        if (this.pending.length > 0) {
          this.pending = this.pending.slice(0, -1);
        }
        this.expectedBackspaceEchoes += 1;
        continue;
      }

      this.pending += ch;
    }

    if (this.pending.length > 4096) {
      this.pending = this.pending.slice(-2048);
    }
  }

  /** Returns true when inbound bytes duplicate locally echoed input. */
  shouldSkipInbound(data: Uint8Array): boolean {
    const chunk = new TextDecoder().decode(data);
    if (!chunk) return false;

    if (this.expectedBackspaceEchoes > 0 && isBackspaceEcho(chunk)) {
      const count = countPureBackspaceEchoes(chunk);
      if (count > 0) {
        this.expectedBackspaceEchoes = Math.max(0, this.expectedBackspaceEchoes - count);
        return true;
      }
    }

    if (!this.pending.length) return false;

    if (this.pending.startsWith(chunk)) {
      this.pending = this.pending.slice(chunk.length);
      return true;
    }

    if (chunk.startsWith(this.pending)) {
      this.pending = '';
      return false;
    }

    if (/[\r\n]/.test(chunk)) {
      this.pending = '';
    }
    return false;
  }

  reset(): void {
    this.pending = '';
    this.expectedBackspaceEchoes = 0;
  }
}
