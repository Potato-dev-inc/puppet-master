import { describe, expect, it, vi } from 'vitest';
import type { StartRequest } from './model';
import { runStart, type StartDeps } from './start';

const req = (extra: Partial<StartRequest> = {}): StartRequest => ({ agentId: 'alpha', folder: '/work/p', isolated: false, runAs: 'terminal', prompt: 'fix it', ...extra });

function deps(extra: Partial<StartDeps> = {}) {
  return {
    newAgent: vi.fn(async (): Promise<string | null> => 'pane-1'),
    startChat: vi.fn(async () => 'chat-1'),
    select: vi.fn(),
    send: vi.fn(async (): Promise<unknown> => undefined),
    notify: vi.fn(),
    ...extra,
  };
}

describe('runStart', () => {
  it('starts a terminal pane the way the + menu does, then queues the first message', async () => {
    const d = deps();
    expect(await runStart(req({ accountId: 'acc', isolated: true }), d)).toBe('pane-1');
    expect(d.newAgent).toHaveBeenCalledWith('alpha', 'acc', true, '/work/p');
    expect(d.send).toHaveBeenCalledWith('pane-1', 'fix it');
    expect(d.startChat).not.toHaveBeenCalled();
  });

  it('starts a chat session in the folder, opens it, and queues the message', async () => {
    const d = deps();
    expect(await runStart(req({ runAs: 'chat' }), d)).toBe('chat-1');
    expect(d.startChat).toHaveBeenCalledWith('alpha', '/work/p');
    expect(d.select).toHaveBeenCalledWith('chat-1');
    expect(d.send).toHaveBeenCalledWith('chat-1', 'fix it');
    expect(d.newAgent).not.toHaveBeenCalled();
  });

  it('a chat without a folder starts nothing and says so', async () => {
    const d = deps();
    expect(await runStart(req({ runAs: 'chat', folder: undefined }), d)).toBeNull();
    expect(d.startChat).not.toHaveBeenCalled();
    expect(d.notify).toHaveBeenCalledWith('Pick a folder for a chat session.');
  });

  it('a chat that cannot start tells the person and sends nothing', async () => {
    const d = deps({ startChat: vi.fn(async () => Promise.reject(new Error('ACP_LABS_OFF: Chat sessions are off.'))) });
    expect(await runStart(req({ runAs: 'chat' }), d)).toBeNull();
    expect(d.notify).toHaveBeenCalledWith('ACP_LABS_OFF: Chat sessions are off.');
    expect(d.send).not.toHaveBeenCalled();
  });

  it('a pane that did not start sends nothing (the starting view says why)', async () => {
    const d = deps({ newAgent: vi.fn(async () => null) });
    expect(await runStart(req(), d)).toBeNull();
    expect(d.send).not.toHaveBeenCalled();
    expect(d.notify).not.toHaveBeenCalled();
  });

  it('keeps the pane when only the first message fails, and says so', async () => {
    const d = deps({ send: vi.fn(async () => Promise.reject('The agent is not running.')) });
    expect(await runStart(req(), d)).toBe('pane-1');
    expect(d.notify).toHaveBeenCalledWith('The agent started, but your message was not sent: The agent is not running.');
  });
});
