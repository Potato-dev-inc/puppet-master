// The saved chat's file names and the desktop-only guard of its command wrappers.
import { describe, expect, it } from 'vitest';
import { chatExportHtml, chatExportMarkdown, chatExportSave, chatPageFileName } from './chatExport';

describe('chatPageFileName', () => {
  it('names the file by the day and the minute it was saved', () => {
    expect(chatPageFileName(new Date(2026, 9, 28, 15, 5))).toBe('chat-2026-10-28-1505.html');
  });

  it('names a Markdown file with .md', () => {
    expect(chatPageFileName(new Date(2026, 9, 28, 15, 5), 'markdown')).toBe('chat-2026-10-28-1505.md');
    expect(chatPageFileName(new Date(2026, 9, 28, 15, 5), 'page')).toBe('chat-2026-10-28-1505.html');
  });
});

describe('chat export commands outside the desktop shell', () => {
  it('refuse to run', async () => {
    await expect(chatExportHtml('pane-1')).rejects.toThrow('Desktop shell required: chat_export_html');
    await expect(chatExportMarkdown('pane-1')).rejects.toThrow('Desktop shell required: chat_export_markdown');
    await expect(chatExportSave('token', 'C:/x.md')).rejects.toThrow('Desktop shell required: chat_export_save');
  });
});
