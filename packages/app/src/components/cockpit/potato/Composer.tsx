// The message box at the bottom of a bot's thread, a rounded pill that grows to about eight lines. Enter sends, Shift+Enter
// adds a line. `@` lists the other bots, `/` at the start lists commands. Files can be attached with the paperclip, by
// pasting, or by dropping them on the box; they show as chips. Each bot keeps its own unsent text. A failed send shows
// a strip above the box with Retry. A quote from the thread (`potato:quote`) lands in the box as `> lines`.
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ChangeEvent,
  type ClipboardEvent,
  type DragEvent,
  type KeyboardEvent,
  type PointerEvent,
} from 'react';
import { COPY, plainError, type PotatoBotView, type PotatoTransport } from '../../../lib/potato';
import { defaultAttachTransport, readAttachment, type AttachTransport } from '../../../lib/potato-attach';
import {
  ATTACH_LIMITS,
  COMPOSER_COPY,
  LINE_PX,
  MAX_LINES,
  MAX_MESSAGE,
  addQuote,
  applyPick,
  composerBlock,
  filterMentions,
  filterSlash,
  helpText,
  mentionAt,
  mentionWord,
  parseCommand,
  potatoDraftKey,
  quoteFromEvent,
  slashAt,
  slashItems,
  wordsRoom,
  type Attachment,
  type MentionTarget,
  type ParsedCommand,
} from '../../../lib/potato-composer';
import { DRAFT_SAVE_MS, clearDraft, readDraft, writeDraft } from '../../../lib/composerDraft';
import { KEYBOARD_MIC_HINT, browserSpeechCtor, insertAtCursor } from '../../../lib/voice';
import { useVoiceInput } from '../../../hooks/useVoiceInput';
import { AttachChips } from './AttachChips';
import { ComposerPicker, pickerOptionId, type PickerItem } from './ComposerPickers';
import '../../../styles/cockpit-potato-composer.css';

export { MAX_MESSAGE } from '../../../lib/potato-composer';

/** What the composer can ask the page to do. A command is offered only when the page passes its action in. */
export interface ComposerActions {
  /** `/new`: make a new bot. */
  onNewBot?: () => void;
  /** `/computer`: open this bot's computer in the panel. */
  onOpenComputer?: () => void;
}

export interface ComposerProps {
  bot: PotatoBotView;
  transport: PotatoTransport;
  /** Called after a send, so the state is read again at once. */
  onChanged: () => void;
  /** Said when the send fails. */
  onNotify: (text: string) => void;
  /** The message went out: the thread follows the bottom again. */
  onSent?: () => void;
  /** The bots `@` offers. Default: the app's bots, read the first time `@` is typed. */
  bots?: MentionTarget[];
  actions?: ComposerActions;
  /** How a message with files is sent. Default: the app's (desktop). `null`: no attaching. */
  attach?: AttachTransport | null;
}

/** One message waiting to be sent again. */
interface Unsent {
  text: string;
  files: Attachment[];
}

interface Failure {
  message: string;
  /** Whether Retry is offered (a failed send, not a rule the person can fix by editing). */
  retry: boolean;
}

function PaperclipIcon() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M13 7.4l-4.6 4.6a2.6 2.6 0 0 1-3.7-3.7l5-5a1.7 1.7 0 0 1 2.4 2.4l-5 5a.8.8 0 0 1-1.2-1.2l4.4-4.4" />
    </svg>
  );
}

function SendIcon() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M8 13V3M3.8 7.2L8 3l4.2 4.2" />
    </svg>
  );
}

function MicIcon() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="5.7" y="1.8" width="4.6" height="8" rx="2.3" />
      <path d="M3.4 7.6a4.6 4.6 0 0 0 9.2 0M8 12.2v2" />
    </svg>
  );
}

function AlertIcon() {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
      <circle cx="8" cy="8" r="6.2" />
      <path d="M8 4.8v3.8M8 11.1v.1" />
    </svg>
  );
}

const sameFiles = (a: readonly Attachment[], b: readonly Attachment[]) => a.length === b.length && a.every((f, i) => f.id === b[i].id);

/** One box per bot: switching bots remounts it, so each bot shows its own unsent text. */
export function Composer(props: ComposerProps) {
  return <ComposerBox key={props.bot.id} {...props} />;
}

function ComposerBox({ bot, transport, onChanged, onNotify, onSent, bots: botsProp, actions, attach }: ComposerProps) {
  const key = potatoDraftKey(bot.id);
  const blocked = composerBlock(bot);
  const attachApi = attach === undefined ? defaultAttachTransport() : (attach ?? undefined);
  const support = { newBot: !!actions?.onNewBot, computer: !!actions?.onOpenComputer };
  const listId = useId();

  // The text and the files are also kept in refs that change at once, so code that runs after an await sees them.
  const [draft, setDraftState] = useState(() => readDraft(key));
  const draftRef = useRef(draft);
  const [files, setFilesState] = useState<Attachment[]>([]);
  const filesRef = useRef<Attachment[]>([]);
  const [caret, setCaret] = useState(draft.length);
  const [sending, setSending] = useState(false);
  const sendingRef = useRef(false);
  const [error, setError] = useState<Failure | null>(null);
  const unsent = useRef<Unsent | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [dismissedAt, setDismissedAt] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const [botList, setBotList] = useState<MentionTarget[] | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const caretAfter = useRef<number | null>(null);

  const setDraft = useCallback((value: string) => {
    draftRef.current = value;
    setDraftState(value);
  }, []);
  const setFiles = useCallback((next: Attachment[]) => {
    filesRef.current = next;
    setFilesState(next);
  }, []);

  // ---- grow with the text, up to eight lines ---------------------------------------------------
  useLayoutEffect(() => {
    const el = field.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, LINE_PX * MAX_LINES)}px`;
  }, [draft]);

  useLayoutEffect(() => {
    const el = field.current;
    if (!el || caretAfter.current === null) return;
    el.setSelectionRange(caretAfter.current, caretAfter.current);
    setCaret(caretAfter.current);
    caretAfter.current = null;
  });

  // ---- the draft: written a moment after the last change, and when the box goes away -----------
  const dirty = useRef(false);
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    dirty.current = true;
    const timer = window.setTimeout(() => {
      dirty.current = false;
      writeDraft(key, draft);
    }, DRAFT_SAVE_MS);
    return () => window.clearTimeout(timer);
  }, [draft, key]);
  useEffect(
    () => () => {
      if (dirty.current) writeDraft(key, draftRef.current);
    },
    [key],
  );

  // ---- a quote from the thread ----------------------------------------------------------------
  useEffect(() => {
    const onQuote = (event: Event) => {
      const text = quoteFromEvent((event as CustomEvent).detail, bot.id);
      if (text === null || text.trim() === '' || blocked) return;
      const next = addQuote(draftRef.current, text);
      setDraft(next);
      caretAfter.current = next.length;
      field.current?.focus();
    };
    window.addEventListener('potato:quote', onQuote);
    return () => window.removeEventListener('potato:quote', onQuote);
  }, [bot.id, blocked, setDraft]);

  // ---- the pickers -----------------------------------------------------------------------------
  const slash = blocked ? null : slashAt(draft, caret);
  const mention = blocked || slash ? null : mentionAt(draft, caret);
  const commandItems = slash ? filterSlash(slashItems(support), slash.query) : [];
  // A command typed out in full needs no list: Enter runs it.
  const exact = slash && commandItems.length === 1 && commandItems[0].text === draft && caret === draft.length;
  const mentionBots = botsProp ?? botList ?? [];
  const mentionItems = mention ? filterMentions(mentionBots, mention.query, bot.id) : [];
  const wantsBots = mention !== null && botsProp === undefined && botList === null;
  const items: PickerItem[] = slash
    ? commandItems.map((c) => ({ id: c.text, label: c.text, hint: c.hint }))
    : mentionItems.map((b) => ({ id: b.id, label: mentionWord(b) }));
  // `@` with nobody else to list says so, instead of staying silent.
  const mentionLoaded = botsProp !== undefined || botList !== null;
  const nobody = mention !== null && mention.query === '' && mentionLoaded && items.length === 0;
  const pickerOpen = dismissedAt !== draft && !exact && (items.length > 0 || nobody);
  const activeIndex = Math.min(active, Math.max(0, items.length - 1));
  const pickerKey = `${slash ? 's' : mention ? 'm' : ''}:${slash?.query ?? mention?.query ?? ''}`;
  useEffect(() => setActive(0), [pickerKey]);

  // The other bots are read once, the first time `@` is typed.
  useEffect(() => {
    if (!wantsBots) return;
    let live = true;
    transport
      .state()
      .then((s) => live && setBotList(s.bots.map((b) => ({ id: b.id, name: b.name }))))
      .catch(() => live && setBotList([]));
    return () => {
      live = false;
    };
  }, [wantsBots, transport]);

  const pick = (item: PickerItem) => {
    const trigger = slash ?? mention;
    if (!trigger) return;
    const next = applyPick(draftRef.current, caret, trigger, item.label, !slash);
    setDraft(next.value);
    caretAfter.current = next.caret;
    field.current?.focus();
  };

  // ---- files -----------------------------------------------------------------------------------
  const addFiles = async (list: File[]) => {
    if (!attachApi || blocked || list.length === 0) return;
    const rejected: string[] = [];
    for (const file of list) {
      const read = await readAttachment(file, filesRef.current);
      if (read.ok) setFiles([...filesRef.current, read.attachment]);
      else rejected.push(read.reason);
    }
    setNotice(rejected.length === 0 ? null : rejected.length === 1 ? rejected[0] : `${rejected[0]} (${rejected.length - 1} more not added.)`);
    field.current?.focus();
  };

  const onChooseFiles = (e: ChangeEvent<HTMLInputElement>) => {
    const chosen = Array.from(e.target.files ?? []);
    e.target.value = '';
    void addFiles(chosen);
  };

  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    if (!attachApi) return;
    const pasted = Array.from(e.clipboardData?.files ?? []);
    if (pasted.length === 0) return;
    e.preventDefault();
    void addFiles(pasted);
  };

  const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes('Files');
  const onDragOver = (e: DragEvent) => {
    if (!attachApi || blocked || !hasFiles(e)) return;
    e.preventDefault();
    setDragging(true);
  };
  const onDragLeave = (e: DragEvent) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false);
  };
  const onDrop = (e: DragEvent) => {
    if (!attachApi || blocked || !hasFiles(e)) return;
    e.preventDefault();
    setDragging(false);
    void addFiles(Array.from(e.dataTransfer.files));
  };

  // ---- the mic (only where the browser can listen on the device) -------------------------------
  const [speechPossible] = useState(() => browserSpeechCtor() !== null);
  const mic = useVoiceInput({
    enabled: speechPossible,
    onInterim: () => {},
    onFinal: (chunk) => {
      const el = field.current;
      const value = draftRef.current;
      const start = el ? el.selectionStart : value.length;
      const end = el ? el.selectionEnd : value.length;
      const next = insertAtCursor(value, start, end, chunk);
      setDraft(next.value);
      caretAfter.current = next.caret;
    },
  });
  const micOff = !!blocked || sending || mic.support === 'checking';
  const micNote = mic.note && mic.note !== KEYBOARD_MIC_HINT ? mic.note : null;
  const onMicDown = (e: PointerEvent<HTMLButtonElement>) => {
    e.preventDefault();
    if (!micOff) mic.pressStart();
  };
  const onMicKey = (e: KeyboardEvent<HTMLButtonElement>) => {
    if ((e.key !== 'Enter' && e.key !== ' ') || e.repeat) return;
    e.preventDefault();
    if (!micOff) mic.tap();
  };

  // ---- sending ---------------------------------------------------------------------------------
  const send = async (again?: Unsent) => {
    if (sendingRef.current || blocked) return;
    const text = again ? again.text : draftRef.current.trim();
    const sentFiles = again ? again.files : filesRef.current;
    if (text === '' && sentFiles.length === 0) return;
    if (sentFiles.length > 0 && text.length > wordsRoom(sentFiles.length)) {
      setError({ message: COMPOSER_COPY.tooLongWithFiles, retry: false });
      return;
    }
    sendingRef.current = true;
    setSending(true);
    setError(null);
    setNotice(null);
    unsent.current = null;
    mic.halt();
    // Sent from the box, or sent again with the box still holding the same message: the box empties.
    if (!again || (draftRef.current.trim() === again.text && sameFiles(filesRef.current, again.files))) {
      setDraft('');
      setFiles([]);
      dirty.current = false;
      clearDraft(key);
    }
    onSent?.();
    try {
      if (sentFiles.length > 0) await attachApi!.sendWithFiles(bot.id, text, sentFiles);
      else await transport.send(bot.id, text);
      onChanged();
    } catch (cause) {
      const message = plainError(cause, COMPOSER_COPY.sendFailed);
      unsent.current = { text, files: sentFiles };
      // The words come back so nothing is lost; if the person has already started something new, that stays.
      if (draftRef.current.trim() === '' && filesRef.current.length === 0) {
        setDraft(text);
        setFiles(sentFiles);
        caretAfter.current = text.length;
      }
      setError({ message, retry: true });
      onNotify(message);
    } finally {
      sendingRef.current = false;
      setSending(false);
      field.current?.focus();
    }
  };

  const retry = () => {
    if (unsent.current) void send(unsent.current);
  };

  const runCommand = async (command: ParsedCommand) => {
    const done = () => {
      setDraft('');
      dirty.current = false;
      clearDraft(key);
    };
    switch (command.name) {
      case 'help':
        done();
        setNotice(helpText(support));
        return;
      case 'new':
        done();
        actions?.onNewBot?.();
        return;
      case 'computer':
        done();
        actions?.onOpenComputer?.();
        return;
      case 'persona': {
        if (command.arg !== 'fish' && command.arg !== 'potato') {
          setNotice(COMPOSER_COPY.persona);
          return;
        }
        done();
        try {
          await transport.setPersona(bot.id, command.arg);
          onChanged();
        } catch (cause) {
          const message = plainError(cause, 'Could not change the voice.');
          setError({ message, retry: false });
          onNotify(message);
        }
      }
    }
  };

  const submit = () => {
    if (filesRef.current.length === 0) {
      const command = parseCommand(draftRef.current, support);
      if (command) {
        void runCommand(command);
        return;
      }
    }
    void send();
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (pickerOpen) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const step = e.key === 'ArrowDown' ? 1 : -1;
        setActive((activeIndex + step + items.length) % Math.max(1, items.length));
        return;
      }
      if ((e.key === 'Enter' && !e.shiftKey) || e.key === 'Tab') {
        e.preventDefault();
        if (items.length > 0) pick(items[activeIndex]);
        else setDismissedAt(draft);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setDismissedAt(draft);
        return;
      }
    }
    if (e.key !== 'Enter' || e.shiftKey) return;
    e.preventDefault();
    submit();
  };

  const syncCaret = () => setCaret(field.current?.selectionStart ?? draftRef.current.length);

  const empty = draft.trim() === '' && files.length === 0;
  const reasonId = `${listId}-reason`;

  return (
    <form
      className={dragging ? 'ck-potato-composer ck-potato-composer--drop' : 'ck-potato-composer'}
      data-testid="potato-composer"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      onDragEnter={onDragOver}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
    >
      {error && (
        <div className="ck-potato-strip ck-potato-strip--error" role="alert">
          <AlertIcon />
          <span className="ck-potato-strip-text">{error.message}</span>
          {error.retry && (
            <button type="button" className="ck-potato-strip-btn" onClick={retry} disabled={sending}>
              {COMPOSER_COPY.retry}
            </button>
          )}
          <button type="button" className="ck-potato-strip-x" aria-label={COMPOSER_COPY.dismiss} onClick={() => setError(null)}>
            ×
          </button>
        </div>
      )}
      {notice && (
        <div className="ck-potato-strip" role="status">
          <span className="ck-potato-strip-text">{notice}</span>
          <button type="button" className="ck-potato-strip-x" aria-label={COMPOSER_COPY.dismiss} onClick={() => setNotice(null)}>
            ×
          </button>
        </div>
      )}
      <AttachChips files={files} onRemove={(id) => setFiles(filesRef.current.filter((f) => f.id !== id))} />
      <div className="ck-potato-pillwrap">
        {pickerOpen && (
          <ComposerPicker
            id={`${listId}-list`}
            label={slash ? 'Commands' : 'Bots'}
            items={items}
            active={activeIndex}
            empty={COMPOSER_COPY.noOthers}
            onPick={pick}
            onHover={setActive}
          />
        )}
        <div className="ck-potato-pill">
          {attachApi && (
            <>
              <button
                type="button"
                className="ck-potato-round"
                aria-label={COMPOSER_COPY.attach}
                title={COMPOSER_COPY.attach}
                disabled={!!blocked || files.length >= ATTACH_LIMITS.files}
                onClick={() => fileInput.current?.click()}
              >
                <PaperclipIcon />
              </button>
              <input ref={fileInput} type="file" multiple hidden data-testid="potato-file-input" onChange={onChooseFiles} />
            </>
          )}
          <textarea
            ref={field}
            className="ck-potato-field"
            aria-label={COPY.composer(bot.name)}
            placeholder={COPY.composer(bot.name)}
            aria-describedby={blocked ? reasonId : undefined}
            aria-autocomplete="list"
            aria-controls={pickerOpen && items.length > 0 ? `${listId}-list` : undefined}
            aria-activedescendant={pickerOpen && items.length > 0 ? pickerOptionId(`${listId}-list`, activeIndex) : undefined}
            rows={1}
            maxLength={MAX_MESSAGE}
            value={draft}
            disabled={!!blocked}
            onChange={(e) => {
              setDraft(e.target.value);
              setCaret(e.target.selectionStart ?? e.target.value.length);
              if (error && !error.retry) setError(null);
            }}
            onSelect={syncCaret}
            onClick={syncCaret}
            onKeyUp={syncCaret}
            onKeyDown={onKey}
            onPaste={onPaste}
          />
          {speechPossible && mic.support === 'on-device' && (
            <button
              type="button"
              className="ck-potato-round"
              data-testid="potato-mic"
              aria-label="Hold to talk"
              aria-pressed={mic.recording}
              aria-disabled={micOff || undefined}
              onPointerDown={onMicDown}
              onPointerUp={() => mic.pressEnd()}
              onPointerCancel={() => mic.pressEnd()}
              onPointerLeave={() => mic.pressEnd()}
              onKeyDown={onMicKey}
            >
              <MicIcon />
            </button>
          )}
          <button type="submit" className="ck-potato-send" disabled={sending || empty || !!blocked}>
            <SendIcon />
            <span className="ck-potato-sr">Send</span>
          </button>
        </div>
      </div>
      {blocked && (
        <p id={reasonId} className="ck-potato-reason" role="status">
          {blocked}
        </p>
      )}
      {micNote && (
        <p className="ck-potato-reason" role="status">
          {micNote}
        </p>
      )}
      {dragging && <div className="ck-potato-dropnote" aria-hidden="true">{COMPOSER_COPY.dropHere}</div>}
    </form>
  );
}
