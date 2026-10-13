import { describe, expect, it } from 'vitest';
import { ownerResizeDecision } from './owner-follow';

describe('ownerResizeDecision', () => {
  it('follows a phone-held PTY size and grows the grid to it', () => {
    const decision = ownerResizeDecision(
      { cols: 46, rows: 45 },
      { cols: 121, rows: 36 },
      { cols: 121, rows: 36 },
    );
    expect(decision.follow).toEqual({ cols: 46, rows: 45 });
    expect(decision.resizeTo).toEqual({ cols: 46, rows: 45 });
  });

  it('stops following when the PTY is back at the size the window asked for', () => {
    const decision = ownerResizeDecision(
      { cols: 121, rows: 36 },
      { cols: 121, rows: 36 },
      { cols: 46, rows: 45 },
    );
    expect(decision.follow).toBeNull();
    expect(decision.resizeTo).toEqual({ cols: 121, rows: 36 });
  });

  it('does nothing when the grid already matches the PTY and the window owns the size', () => {
    const decision = ownerResizeDecision(
      { cols: 121, rows: 36 },
      { cols: 121, rows: 36 },
      { cols: 121, rows: 36 },
    );
    expect(decision).toEqual({ follow: null, resizeTo: null });
  });

  it('keeps following without resizing when the grid already shows the held size', () => {
    const decision = ownerResizeDecision(
      { cols: 46, rows: 45 },
      { cols: 121, rows: 36 },
      { cols: 46, rows: 45 },
    );
    expect(decision).toEqual({ follow: { cols: 46, rows: 45 }, resizeTo: null });
  });

  it('does not alias the event object into the follow state', () => {
    const event = { cols: 46, rows: 45 };
    const decision = ownerResizeDecision(event, { cols: 121, rows: 36 }, { cols: 121, rows: 36 });
    event.cols = 1;
    expect(decision.follow).toEqual({ cols: 46, rows: 45 });
  });
});
