// @vitest-environment node
// The rules behind the Potato Bot message box: what turns it off, the `@` and `/` pickers, the attachment caps, quotes.
import { describe, expect, it } from 'vitest';
import {
  ATTACH_LIMITS,
  addQuote,
  applyPick,
  attachKind,
  checkAttach,
  composerBlock,
  filterMentions,
  filterSlash,
  helpText,
  mentionAt,
  mentionWord,
  parseCommand,
  potatoDraftKey,
  quoteBlock,
  quoteFromEvent,
  sizeWord,
  slashAt,
  slashItems,
  wordsRoom,
} from './potato-composer';

describe('composerBlock', () => {
  it('turns the box off with a reason when the bot is switched off or out of its limit, and not otherwise', () => {
    expect(composerBlock({ brainState: 'off' })).toContain('switched off');
    expect(composerBlock({ brainState: 'daily_limit' })).toContain("Today's limit");
    for (const state of ['ready', 'idle', 'stopped', 'needs_key', 'agents'] as const) expect(composerBlock({ brainState: state })).toBeNull();
  });
});

describe('drafts', () => {
  it('keeps one draft per bot', () => {
    expect(potatoDraftKey('fish')).toBe('potato:fish');
    expect(potatoDraftKey('fish')).not.toBe(potatoDraftKey('nimbus'));
  });
});

describe('attachments', () => {
  it('knows pictures and text from the type, and from the name when the type is empty', () => {
    expect(attachKind({ name: 'a.png', type: 'image/png' })).toBe('image');
    expect(attachKind({ name: 'a.svg', type: 'image/svg+xml' })).toBeNull();
    expect(attachKind({ name: 'a.txt', type: 'text/plain' })).toBe('text');
    expect(attachKind({ name: 'a.json', type: 'application/json' })).toBe('text');
    expect(attachKind({ name: 'main.rs', type: '' })).toBe('text');
    expect(attachKind({ name: 'main.rs', type: 'application/octet-stream' })).toBe('text');
    expect(attachKind({ name: 'photo.JPG', type: '' })).toBe('image');
    expect(attachKind({ name: 'run.exe', type: 'application/x-msdownload' })).toBeNull();
    expect(attachKind({ name: 'archive.zip', type: '' })).toBeNull();
  });

  it('takes a file inside the caps', () => {
    expect(checkAttach([], { name: 'a.txt', type: 'text/plain', size: 10 })).toEqual({ ok: true, kind: 'text' });
  });

  it('refuses a fifth file, a wrong kind, an empty file, a big file and too much together, each with a sentence', () => {
    const four = Array.from({ length: ATTACH_LIMITS.files }, () => ({ size: 1 }));
    expect(checkAttach(four, { name: 'a.txt', type: 'text/plain', size: 1 })).toMatchObject({ ok: false, reason: 'You can attach up to 4 files.' });
    expect(checkAttach([], { name: 'a.exe', type: '', size: 1 })).toMatchObject({ ok: false, reason: expect.stringContaining('only pictures') });
    expect(checkAttach([], { name: 'a.txt', type: 'text/plain', size: 0 })).toMatchObject({ ok: false, reason: 'a.txt is empty.' });
    expect(checkAttach([], { name: 'big.png', type: 'image/png', size: ATTACH_LIMITS.fileBytes + 1 })).toMatchObject({ ok: false, reason: 'big.png is bigger than 2.0 MB.' });
    const two = [{ size: ATTACH_LIMITS.fileBytes }, { size: ATTACH_LIMITS.fileBytes - 10 }];
    expect(checkAttach(two, { name: 'c.txt', type: 'text/plain', size: 100 })).toMatchObject({ ok: false, reason: 'The files together are bigger than 4.0 MB.' });
  });

  it('says sizes briefly and leaves room for the file lines in the words', () => {
    expect(sizeWord(12)).toBe('12 B');
    expect(sizeWord(2048)).toBe('2 KB');
    expect(sizeWord(1_572_864)).toBe('1.5 MB');
    expect(wordsRoom(0)).toBe(4000);
    expect(wordsRoom(2)).toBeLessThan(4000);
  });
});

describe('the @ picker', () => {
  it('opens on @ at the start or after a space or line break, and not inside a word or an address', () => {
    expect(mentionAt('@', 1)).toEqual({ start: 0, query: '' });
    expect(mentionAt('hi @ni', 6)).toEqual({ start: 3, query: 'ni' });
    expect(mentionAt('hi\n@x', 5)).toEqual({ start: 3, query: 'x' });
    expect(mentionAt('me@site.com', 11)).toBeNull();
    expect(mentionAt('@nimbus hello', 13)).toBeNull();
    expect(mentionAt('hello', 5)).toBeNull();
  });

  it('lists the other bots: names that start with it first, then names that contain it; never the bot itself', () => {
    const bots = [
      { id: 'fish', name: 'Fish' },
      { id: 'nimbus', name: 'Nimbus' },
      { id: 'mochi', name: 'Mochi' },
      { id: 'sushi', name: 'Sushi' },
    ];
    expect(filterMentions(bots, '', 'fish').map((b) => b.id)).toEqual(['nimbus', 'mochi', 'sushi']);
    expect(filterMentions(bots, 'm', 'fish').map((b) => b.id)).toEqual(['mochi', 'nimbus']);
    expect(filterMentions(bots, 'F', 'fish')).toEqual([]);
    expect(mentionWord({ id: 'a', name: 'Big Fish' })).toBe('@BigFish');
  });

  it('puts the picked name where the word was, with a space after it, and the caret after that', () => {
    const picked = applyPick('hi @ni there', 6, { start: 3, query: 'ni' }, '@Nimbus');
    expect(picked.value).toBe('hi @Nimbus there');
    expect(picked.caret).toBe(10);
    expect(applyPick('@n', 2, { start: 0, query: 'n' }, '@Nimbus')).toEqual({ value: '@Nimbus ', caret: 8 });
  });
});

describe('the / picker and commands', () => {
  it('opens only for a slash at the very start, on the first line', () => {
    expect(slashAt('/', 1)).toEqual({ start: 0, query: '' });
    expect(slashAt('/per', 4)).toEqual({ start: 0, query: 'per' });
    expect(slashAt('/persona f', 10)).toEqual({ start: 0, query: 'persona f' });
    expect(slashAt('hi /new', 7)).toBeNull();
    expect(slashAt('/a\nb', 4)).toBeNull();
  });

  it('offers only the commands the page can run', () => {
    expect(slashItems().map((i) => i.text)).toEqual(['/persona fish', '/persona potato', '/help']);
    expect(slashItems({ newBot: true, computer: true }).map((i) => i.text)).toEqual(['/new', '/persona fish', '/persona potato', '/computer', '/help']);
    expect(filterSlash(slashItems(), 'per').map((i) => i.text)).toEqual(['/persona fish', '/persona potato']);
    expect(filterSlash(slashItems(), 'persona p').map((i) => i.text)).toEqual(['/persona potato']);
    expect(filterSlash(slashItems(), 'zzz')).toEqual([]);
    expect(helpText()).toContain('/persona fish');
    expect(helpText()).not.toContain('/computer');
  });

  it('reads a whole message as a command only for a command that exists here', () => {
    expect(parseCommand('/persona fish')).toEqual({ name: 'persona', arg: 'fish' });
    expect(parseCommand('  /Persona   POTATO ')).toEqual({ name: 'persona', arg: 'potato' });
    expect(parseCommand('/help')).toEqual({ name: 'help', arg: '' });
    expect(parseCommand('/new')).toBeNull();
    expect(parseCommand('/new', { newBot: true })).toEqual({ name: 'new', arg: '' });
    expect(parseCommand('/computer')).toBeNull();
    expect(parseCommand('/computer', { computer: true })).toEqual({ name: 'computer', arg: '' });
    expect(parseCommand('/clear')).toBeNull();
    expect(parseCommand('/usr/bin/env')).toBeNull();
    expect(parseCommand('hello /help')).toBeNull();
    expect(parseCommand('/help\nplease')).toBeNull();
  });
});

describe('quotes', () => {
  it('turns every line into a "> " line and leaves a blank line to write in', () => {
    expect(quoteBlock('one\ntwo')).toBe('> one\n> two\n\n');
    expect(quoteBlock('one\r\n\r\ntwo')).toBe('> one\n>\n> two\n\n');
  });

  it('adds the quote under what is already written, and ignores an empty quote', () => {
    expect(addQuote('', 'hi')).toBe('> hi\n\n');
    expect(addQuote('my reply  ', 'hi')).toBe('my reply\n\n> hi\n\n');
    expect(addQuote('x', '   ')).toBe('x');
  });

  it('reads the event text, and drops a quote meant for another bot', () => {
    expect(quoteFromEvent('plain', 'fish')).toBe('plain');
    expect(quoteFromEvent({ text: 'a' }, 'fish')).toBe('a');
    expect(quoteFromEvent({ text: 'a', botId: 'fish' }, 'fish')).toBe('a');
    expect(quoteFromEvent({ text: 'a', botId: 'nimbus' }, 'fish')).toBeNull();
    expect(quoteFromEvent(null, 'fish')).toBeNull();
    expect(quoteFromEvent({ text: 5 }, 'fish')).toBeNull();
  });
});
