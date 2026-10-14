// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { PreviewNote } from './PreviewNote';
import { StuckNote } from './StuckNote';
import { InputRefusedNote } from './InputRefusedNote';
import { AGENT_RUN_BY_ANOTHER_AGENT, inputRefusalNote } from '../../lib/input-refusal';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('phone notes', () => {
  it('StuckNote says the agent looks stuck and gives the summary', async () => {
    await act(async () => {
      root.render(<StuckNote signal={{ kind: 'repeated_error', summary: 'the same error 4 times' }} />);
    });
    const note = host.querySelector('[role="note"]') as HTMLElement;
    expect(note).not.toBeNull();
    expect(note.textContent).toContain('Looks stuck');
    expect(note.textContent).toContain('the same error 4 times');
  });

  it('PreviewNote shows the dev server address and that it opens on the PC', async () => {
    await act(async () => {
      root.render(<PreviewNote preview={{ url: 'http://localhost:5173', port: 5173 }} />);
    });
    const note = host.querySelector('[role="note"]') as HTMLElement;
    expect(note).not.toBeNull();
    expect(note.textContent).toContain('Preview');
    expect(note.textContent).toContain('http://localhost:5173');
    expect(note.textContent).toContain('Open on your PC');
  });

  it('InputRefusedNote shows why typing was refused, in plain words', async () => {
    await act(async () => {
      root.render(<InputRefusedNote text={inputRefusalNote('managed pane mutations require an MCP session id')} />);
    });
    const note = host.querySelector('[data-testid="input-refused-note"]') as HTMLElement;
    expect(note).not.toBeNull();
    expect(note.getAttribute('role')).toBe('status');
    expect(note.textContent).toBe(AGENT_RUN_BY_ANOTHER_AGENT);
  });
});
