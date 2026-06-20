import { backspaceVisualErase, isBackspaceEcho, isBackspaceKey } from './mirror-local-echo';

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
  private visibleInput = '';
  private expectedBackspaceEchoes = 0;
  private confirmedOnPty = 0;

  noteOutbound(text: string): void {
    for (const ch of text) {
      if (isBackspaceKey(ch)) {
        this.noteBackspaceForEcho();
        continue;
      }

      this.notePrintable(ch);
    }
  }

  /**
   * Track an outbound backspace and return visual erase bytes when a local echo exists.
   * When pending is empty, the backspace is still forwarded to the PTY but must not
   * visually erase prompt/scrollback that only exists from remote output.
   */
  noteBackspaceForEcho(): string {
    const visibleChars = Array.from(this.visibleInput);
    const removed = visibleChars.pop();
    if (removed) {
      this.visibleInput = visibleChars.join('');
      this.expectedBackspaceEchoes += 1;

      const pendingChars = Array.from(this.pending);
      if (pendingChars.length > 0) {
        pendingChars.pop();
        this.pending = pendingChars.join('');
      } else if (this.confirmedOnPty > 0) {
        this.confirmedOnPty -= 1;
      }

      return backspaceVisualErase(removed);
    }

    if (this.confirmedOnPty > 0) {
      this.confirmedOnPty -= 1;
    }

    // No user-originated visible text remains; do not backspace into prompt output.
    return '';
  }

  notePrintable(text: string): void {
    if (!text) return;
    for (const ch of text) {
      if (ch === '\r' || ch === '\n') {
        this.pending = '';
        this.visibleInput = '';
        this.confirmedOnPty = 0;
        continue;
      }

      this.pending += ch;
      this.visibleInput += ch;
    }
    if (this.pending.length > 4096) {
      this.pending = this.pending.slice(-2048);
    }
    if (this.visibleInput.length > 4096) {
      this.visibleInput = this.visibleInput.slice(-2048);
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
      this.confirmedOnPty += Array.from(chunk).length;
      return true;
    }

    if (chunk.startsWith(this.pending)) {
      if (this.pending.length > 0) {
        this.confirmedOnPty += Array.from(this.pending).length;
      }
      this.pending = '';
      return false;
    }

    if (/[\r\n]/.test(chunk)) {
      this.pending = '';
      this.visibleInput = '';
      this.confirmedOnPty = 0;
    }
    return false;
  }

  reset(): void {
    this.pending = '';
    this.visibleInput = '';
    this.expectedBackspaceEchoes = 0;
    this.confirmedOnPty = 0;
  }
}
