# Spaces D-4: channels and chat UI

Read `20-spaces-overview.md` and the shared rules R1-R7 at the top of
`21-spaces-d0-d1-transport-spike-and-secure-core.md`. R1-R7 apply in full to every slice here.

Writer's sources: `00-FORMAT.md` and `_facts/spaces-facts.md` only (facts cited as `facts §N`).

Slices: D4.1, D4.2, D4.3, D4.4, D4.5 (5 slices).

UI rules for every slice here:
- Monochrome only (grey scale), Cursor/Codex density, no coloured glass (repo rule; `docs/design/`).
- Class prefix `ck-spaces-`, modifiers `is-<state>` (facts §11: `ck-desk-*`, `is-on`, `is-error`).
- One stylesheet: `packages/app/src/styles/cockpit-spaces.css`, imported by `SpacesPage.tsx` like
  `DeskPage.tsx` imports `cockpit-desk.css` (facts §11).
- Message text is plain text: no HTML, no Markdown rendering, no automatic links in v1. New lines kept with
  `white-space: pre-wrap`.
- Every component has a test file next to it (facts §11). Vitest runs from the repo root (facts §10).

## Page layout (all of D-4)

```
+------------+---------------+-------------------------------------+--------------+
| Spaces     | Book club  v  | # general                           | People (4)   |
| (rail)     | channels      | ----------------------------------- | o Ren  owner |
|            | # general  3  | Sam 10:02                           | o Sam        |
| Book club 3| # books       |   hi all                            | . Ana        |
| Running    | + New channel | Fish (Ren's) 10:03                  | . Bo         |
|            |               |   ...                               |              |
| + New space|               | [Message #general           ] Send  |              |
| Join space |               |                                     |              |
+------------+---------------+-------------------------------------+--------------+
```
Rail (`SpaceList`, D4.2) | channel column (`ChannelList`, D4.4) | feed (`ChannelView` + `Composer`, D4.3) |
people (`MemberList`, D4.5). Below 900 px wide the people column is hidden behind a `People` button in the header.

## Words on screen (all of D-4, exact)

| Place | Text |
|---|---|
| Sidebar row | `Spaces` |
| Page title, no spaces yet | `Spaces` |
| Empty page body | `Chat with friends in small groups, computer to computer. No server keeps your messages.` |
| Buttons on rail | `New space`, `Join a space` |
| No space picked | `Pick a space on the left.` |
| Offline banner | `You're offline. New messages will go out when you're back online.` |
| Nobody else online | `No one else is online right now. New messages wait until someone is.` |
| Relayed line | `Connected through a relay. Messages stay encrypted.` |
| Composer placeholder | `Message #{channel}` |
| Character counter (shown above 3500) | `{n} / 4000` |
| Too long | `This message is too long. The limit is 4000 characters.` |
| Reply bar | `Replying to {name}` + button `Cancel reply` |
| Load older | `Show older messages` |
| Start of channel | `This is the start of #{channel}.` |
| Space info note | `Messages live on members' computers. If everyone is offline, new messages wait. This computer keeps the last 90 days, up to 200 MB per space.` |
| New space sheet | title `New space`, field `Name`, placeholder `Book club`, help `You can change it later.`, buttons `Cancel`, `Create` |
| New channel sheet | title `New channel`, field `Name`, placeholder `books`, help `Lowercase letters, numbers and dashes.`, buttons `Cancel`, `Create` |
| Channel limit | `This space has 20 channels. Archive one first.` |
| Member limit (header line on invite button tooltip) | `This space is full (50 people).` |
| Header buttons | `Invite people` (owner), `People` (narrow screens), `Space info` |
| Presence labels | `Online`, `Away`, `Offline` |
| Fish author | `Fish ({owner}'s)` |
| Member roles | `Owner` (label), members have no label |

---

## D4.1 Commands and TS wrappers for detail, messages, send, channels
Needs merged first: D3.5 (plan 23), D2.7 (plan 22).

### Goal
Add the Tauri commands and service methods the chat UI needs (space detail, message pages, send, read marks,
channel create/rename/archive) and their TS types, wrappers and hooks.

### Read first
- `spaces/commands.rs`, `spaces/service.rs`, `spaces/store.rs` (`messages_page`, `reactions_for`, `unread`,
  `mark_read`), `spaces/roster.rs` (`clean_channel_name`, `next_op`).
- `packages/app/src/lib/spaces.ts`, `spaces.test.ts`.

### Files
- edit `packages/app/src-tauri/src/spaces/service.rs`
- edit `packages/app/src-tauri/src/spaces/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- edit `packages/app/src/lib/spaces.ts`
- edit `packages/app/src/lib/spaces.test.ts`

### Steps
1. `service.rs`: add the methods below.
2. `commands.rs`: add the 7 commands below (same `ready` gate).
3. `lib.rs`: register them after `spaces::commands::space_join_cancel,`.
4. `lib/spaces.ts`: add the types, wrappers, `useSpace`, `useChannel`.
5. Tests.

### Code
Service methods (Rust):
```rust
impl SpacesService {
    pub fn detail(&self, space: &str) -> Result<Value, String>;            // SpaceDetail JSON
    pub fn messages(&self, space: &str, channel: &str, before: Option<&str>, limit: u32) -> Result<Value, String>;
    pub async fn send_text(&self, space: &str, channel: &str, text: &str, reply: Option<&str>) -> Result<Value, String>;
    pub fn mark_read(&self, space: &str, channel: &str) -> Result<(), String>;
    pub async fn channel_create(&self, space: &str, name: &str) -> Result<Value, String>;
    pub async fn channel_rename(&self, space: &str, channel: &str, name: &str) -> Result<Value, String>;
    pub async fn channel_archive(&self, space: &str, channel: &str) -> Result<Value, String>;
}
```
Rules:
- `send_text`: `clean_text(text)`; empty -> `Write something first.`; more than 4000 chars -> `This message is too long.
  The limit is 4000 characters.`; channel must exist and not be archived -> `This channel is archived.`;
  `reply` must be an existing message id in this space, else it is dropped. Mentions: every `@Name` that matches a
  current member's name exactly (case-insensitive) adds that member key (max 20). Calls `send(.., Body::Text{..},
  reply)` and returns the new `SpaceMessage` JSON.
- `messages`: `before` is a message id hex; `limit` clamped to 1..=100; returns `SpaceMessage[]` oldest first.
- `channel_create`: owner only (`Only the owner can add channels.`); `clean_channel_name(name)` none -> `Use letters,
  numbers and dashes.`; 20 active -> `This space has 20 channels. Archive one first.`; name taken -> `There is already
  a channel called #{name}.`; `next_op(AddChannel)`, apply, append, forward the op to live peers by sending
  `ControlOps` on each sync stream (add `fn push_control(&self, space, op)` that writes frame `0x02`).
- Archive: owner only; the first channel -> `The first channel can't be archived.`
- After any change: `changed()`; after a stored message: message notifier `{ space, channel, id }`.

`SpaceMessage` JSON:
```json
{
  "id": "<64 hex>", "channel": "<16 hex>", "member": "<64 hex>", "authorName": "Sam",
  "isFish": false, "isMine": false, "createdAtMs": 0, "text": "hi",
  "replyTo": null, "replyPreview": null,
  "deleted": null,
  "reactions": [{ "code": "thumbs_up", "count": 2, "mine": true }],
  "file": null,
  "blocked": false
}
```
`replyPreview` = `{ "authorName": "Ana", "text": "<first 80 chars>" }` or null. `deleted` = `{ "byName": "Sam",
"byOwner": false }` or null (text is `""` when deleted). `isFish` = kind 7, `authorName` then is `Fish (Ren's)`.
`blocked` = author is blocked by me (D5.2; false until then).

`SpaceDetail` JSON:
```json
{
  "id": "<hex>", "name": "Book club", "isOwner": true, "myMember": "<hex>", "ownerName": "Ren",
  "channels": [{ "id": "<hex>", "name": "general", "archived": false, "unread": 3, "fish": "off" }],
  "members": [{ "member": "<hex>", "name": "Ren", "role": "owner", "presence": "online",
                "blocked": false, "isMe": true, "short": "4F7Q-9K2M" }],
  "openInvites": 1, "relayed": false, "anyoneOnline": true
}
```
`presence` is `"offline"` for everyone except me until D8.3; `anyoneOnline` = at least one live sync link.

Commands:
```rust
#[tauri::command] pub async fn space_detail(app: AppHandle, space: String) -> Result<Value, String>;
#[tauri::command] pub async fn space_messages(app: AppHandle, space: String, channel: String,
                                              before: Option<String>, limit: u32) -> Result<Value, String>;
#[tauri::command] pub async fn space_send(app: AppHandle, space: String, channel: String, text: String,
                                          reply: Option<String>) -> Result<Value, String>;
#[tauri::command] pub async fn space_read(app: AppHandle, space: String, channel: String) -> Result<Value, String>;
#[tauri::command] pub async fn space_channel_create(app: AppHandle, space: String, name: String) -> Result<Value, String>;
#[tauri::command] pub async fn space_channel_rename(app: AppHandle, space: String, channel: String, name: String) -> Result<Value, String>;
#[tauri::command] pub async fn space_channel_archive(app: AppHandle, space: String, channel: String) -> Result<Value, String>;
```

TS additions (`lib/spaces.ts`):
```ts
export type ReactionCode = 'thumbs_up' | 'heart' | 'laugh' | 'wow' | 'sad' | 'party' | 'eyes' | 'check';
export interface SpaceChannel { id: string; name: string; archived: boolean; unread: number; fish: FishMode }
export interface SpaceMember {
  member: string; name: string; role: 'owner' | 'member'; presence: PresenceState;
  blocked: boolean; isMe: boolean; short: string;
}
export interface SpaceDetail {
  id: string; name: string; isOwner: boolean; myMember: string; ownerName: string;
  channels: SpaceChannel[]; members: SpaceMember[]; openInvites: number; relayed: boolean; anyoneOnline: boolean;
}
export interface SpaceReaction { code: ReactionCode; count: number; mine: boolean }
export interface SpaceFile { sha: string; name: string; size: number; have: boolean; state: 'idle' | 'getting' | 'failed' }
export interface SpaceMessage {
  id: string; channel: string; member: string; authorName: string; isFish: boolean; isMine: boolean;
  createdAtMs: number; text: string; replyTo: string | null;
  replyPreview: { authorName: string; text: string } | null;
  deleted: { byName: string; byOwner: boolean } | null;
  reactions: SpaceReaction[]; file: SpaceFile | null; blocked: boolean;
}

export const spaceDetail = (space: string) => invoke<SpaceDetail>('space_detail', { space });
export const spaceMessages = (space: string, channel: string, before: string | null, limit = 50) =>
  invoke<SpaceMessage[]>('space_messages', { space, channel, before, limit });
export const spaceSend = (space: string, channel: string, text: string, reply: string | null) =>
  invoke<SpaceMessage>('space_send', { space, channel, text, reply });
export const spaceRead = (space: string, channel: string) => invoke<SpaceDetail>('space_read', { space, channel });
export const spaceChannelCreate = (space: string, name: string) =>
  invoke<SpaceDetail>('space_channel_create', { space, name });
export const spaceChannelRename = (space: string, channel: string, name: string) =>
  invoke<SpaceDetail>('space_channel_rename', { space, channel, name });
export const spaceChannelArchive = (space: string, channel: string) =>
  invoke<SpaceDetail>('space_channel_archive', { space, channel });

/** Merge a page into a list: by id, keep server order (oldest first), no duplicates. */
export function mergeMessages(list: SpaceMessage[], page: SpaceMessage[]): SpaceMessage[] {
  const byId = new Map<string, SpaceMessage>();
  for (const m of list) byId.set(m.id, m);
  for (const m of page) byId.set(m.id, m);
  const seen = new Set(list.map((m) => m.id));
  const order = [...list.map((m) => m.id), ...page.map((m) => m.id).filter((id) => !seen.has(id))];
  return order.map((id) => byId.get(id)!);
}

export function useSpace(space: string | null): { detail: SpaceDetail | null; error: string | null; reload: () => void };
// loads spaceDetail(space); reloads on 'spaces://changed'; null when space is null.

export function useChannel(space: string | null, channel: string | null): {
  messages: SpaceMessage[]; atStart: boolean; loadOlder: () => Promise<void>;
  send: (text: string, reply: string | null) => Promise<string | null>; error: string | null;
};
// loads the newest 50; on 'spaces://message' with this space+channel, fetches the newest 50 and merges;
// loadOlder() fetches 50 before the first id; atStart = last page had < 50; send returns an error text or null.
// Calls spaceRead(space, channel) after each load while the channel is shown.
```
`mergeMessages` order: the server returns oldest-first pages; when merging a newer page, append ids not yet present
in page order; when merging an older page (from `loadOlder`), call `mergeMessages(page, list)`. It never re-sorts.

### Tests to write
Rust: none new (service methods are covered by D4 UI tests through mocks and by `service_tests` additions below).
Add to `spaces/service_tests.rs`:
| Test | Input | Expected |
|---|---|---|
| `send_text_rules` | empty text; 4001 chars; archived channel | the three error texts above |
| `channel_create_rules` | member (not owner) creates; owner creates `Books!`; 21st | `Only the owner can add channels.`; channel `books`; `This space has 20 channels. Archive one first.` |
| `new_channel_reaches_members` | A creates `#books`; B online | B's detail lists `books` within 2 s |

TS (`spaces.test.ts` additions):
| Test | Input | Expected |
|---|---|---|
| `mergeMessages appends new ids only` | list [a,b], page [b,c] | [a,b,c] |
| `mergeMessages older page goes first` | `mergeMessages([x,y],[a,b])` | [x,y,a,b] |
| `mergeMessages replaces changed rows` | list [a(text 1)], page [a(text 2)] | [a(text 2)] |
| `useChannel loads newest 50 and marks read` | mock invoke | `space_messages` called with `before: null, limit: 50`; then `space_read` |
| `useChannel refetches on message event for its channel only` | emit `spaces://message` for other channel, then for this one | `space_messages` called once more, not twice |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d41 cargo test --lib spaces::service_tests
# expected: test result: ok. 13 passed; 0 failed
cd ../../..
npx vitest run packages/app/src/lib/spaces.test.ts
# expected: Tests  13 passed (13)
```

### Done when
- [ ] 7 new commands registered and gated; 3 new Rust tests and 5 new TS tests pass.

### Do NOT
- R1-R7.
- Do not return message text from `spaces_view` (only counts); text comes only from `space_messages`.
- Do not render or parse Markdown/HTML anywhere.
- Do not let the UI pass a member key for the author; the backend always uses my keys.

### If it fails
1. Tauri `Option<String>` from JS `null`: pass `null` (not `undefined`) for `before` and `reply`.
2. Message event floods refetches: debounce in `useChannel` to one fetch per 150 ms.
3. `new_channel_reaches_members` fails: the op must also be appended on B (B's sync `ApplyControl` path).

---

## D4.2 Sidebar row, `SpacesPage`, `SpaceList`
Needs merged first: D4.1, D2.9 (plan 22).

### Goal
Show a "Spaces" row in the sidebar when Labs `spaces_enabled` is on, open the Spaces page, list spaces in the rail,
and mount the D-2 sheets (invite, join, join request) on the page.

### Read first
- `packages/app/src/CockpitApp.tsx` lines 40-60, 80-90, 160-175, 425-440, 525-540 (desk wiring, facts §11).
- `packages/app/src/components/cockpit/Sidebar.tsx` lines 60-120, 240-250, 455-480 (desk row, facts §11).
- `packages/app/src/components/desk/DeskPage.tsx` (page shell, CSS import, sheet mounting at lines 223 and 253).
- `components/spaces/InviteSheet.tsx`, `JoinSheet.tsx`, `JoinRequestSheet.tsx`, `lib/spaces.ts`.
- Find the `MainView` type (search `type MainView` in `packages/app/src`).

### Files
- edit `packages/app/src/CockpitApp.tsx`
- edit `packages/app/src/components/cockpit/Sidebar.tsx`
- create `packages/app/src/components/spaces/SpacesPage.tsx`
- create `packages/app/src/components/spaces/SpaceList.tsx`
- create `packages/app/src/components/spaces/SpacesPage.test.tsx`
- (exception) edit the file that declares `MainView` if it is not `CockpitApp.tsx`: add `| 'spaces'` only.

### Steps
1. `CockpitApp.tsx`: import `SpacesPage` and `useSpacesEnabled`; `const spacesOn = useSpacesEnabled();` next to
   `deskOn`; add `'spaces'` to `MainView`; pass `spaces={{ enabled: spacesOn }}`, `spacesOpen={view === 'spaces' &&
   spacesOn}`, `onOpenSpaces={() => setView('spaces')}` to `Sidebar` next to the desk props; render
   `) : view === 'spaces' && spacesOn ? ( <SpacesPage />` directly after the desk branch.
2. `Sidebar.tsx`: props `spaces?: { enabled: boolean }`, `spacesOpen?: boolean`, `onOpenSpaces?: () => void`;
   render the row right after the desk row with the same element and classes, label `Spaces`, active when
   `spacesOpen`; add `&& !spacesOpen` to the active check that already has `&& !deskOpen` (line 246 area).
3. Create `SpaceList.tsx` and `SpacesPage.tsx` (below).
4. Tests.

### Code
`SpaceList.tsx`:
```tsx
import type { SpaceSummary } from '../../lib/spaces';

export interface SpaceListProps {
  spaces: SpaceSummary[];
  selected: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  onJoin: () => void;
}

export function SpaceList({ spaces, selected, onSelect, onNew, onJoin }: SpaceListProps) {
  return (
    <nav className="ck-spaces-rail" aria-label="Spaces">
      <ul>
        {spaces.map((s) => (
          <li key={s.id}>
            <button type="button" className={`ck-spaces-rail-item${s.id === selected ? ' is-active' : ''}`}
              aria-current={s.id === selected ? 'page' : undefined} onClick={() => onSelect(s.id)}>
              <span className="ck-spaces-rail-name">{s.name}</span>
              {s.unread > 0 && <span className="ck-spaces-badge" aria-label={`${s.unread} unread`}>{s.unread}</span>}
              <span className="ck-spaces-rail-sub">{s.online > 0 ? `${s.online} online` : 'No one online'}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className="ck-spaces-rail-actions">
        <button type="button" className="ck-spaces-btn" onClick={onNew}>New space</button>
        <button type="button" className="ck-spaces-btn" onClick={onJoin}>Join a space</button>
      </div>
    </nav>
  );
}
```

`SpacesPage.tsx`:
```tsx
// Spaces (Labs `spaces_enabled`): the page.
import { useEffect, useState } from 'react';
import '../../styles/cockpit-spaces.css';
import { useSpaces, useSpace, type SpaceInvite } from '../../lib/spaces';
import { SpaceList } from './SpaceList';
import { InviteSheet } from './InviteSheet';
import { JoinSheet } from './JoinSheet';
import { JoinRequestSheet } from './JoinRequestSheet';

export function SpacesPage() {
  const api = useSpaces(true);
  const view = api.view;
  const [selected, setSelected] = useState<string | null>(null);
  const [joiningOpen, setJoiningOpen] = useState(false);
  const [creating, setCreating] = useState(false);          // NewSpaceSheet comes in D4.5
  const [invite, setInvite] = useState<SpaceInvite | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const { detail } = useSpace(selected);
  useEffect(() => { const t = setInterval(() => setNow(Date.now()), 30_000); return () => clearInterval(t); }, []);
  useEffect(() => {
    if (!selected && view && view.spaces.length > 0) setSelected(view.spaces[0].id);
  }, [view, selected]);
  const joining = view?.joining[0] ?? null;
  const request = view?.requests[0] ?? null;

  return (
    <div className="ck-spaces">
      {view && view.spaces.length === 0 && !joiningOpen ? (
        <section className="ck-spaces-empty">
          <h1 className="ck-spaces-title">Spaces</h1>
          <p>Chat with friends in small groups, computer to computer. No server keeps your messages.</p>
          <div className="ck-spaces-actions">
            <button type="button" className="ck-spaces-btn is-primary" onClick={() => setCreating(true)}>New space</button>
            <button type="button" className="ck-spaces-btn" onClick={() => setJoiningOpen(true)}>Join a space</button>
          </div>
        </section>
      ) : (
        <>
          <SpaceList spaces={view?.spaces ?? []} selected={selected} onSelect={setSelected}
            onNew={() => setCreating(true)} onJoin={() => setJoiningOpen(true)} />
          <main className="ck-spaces-main">
            {view && !view.online && (
              <p className="ck-spaces-banner" role="status">You're offline. New messages will go out when you're back online.</p>
            )}
            {!detail ? (
              <p className="ck-spaces-empty">Pick a space on the left.</p>
            ) : (
              <header className="ck-spaces-head">
                <h1 className="ck-spaces-title">{detail.name}</h1>
                {!detail.anyoneOnline && (
                  <span className="ck-spaces-status">No one else is online right now. New messages wait until someone is.</span>
                )}
                {detail.relayed && <span className="ck-spaces-status">Connected through a relay. Messages stay encrypted.</span>}
                {detail.isOwner && (
                  <button type="button" className="ck-spaces-btn"
                    onClick={async () => { const i = await api.invite(detail.id); if (i) setInvite(i); }}>
                    Invite people
                  </button>
                )}
              </header>
            )}
            {/* D4.4 puts ChannelList + ChannelView here; D4.5 puts MemberList on the right. */}
            {api.error && <p className="ck-spaces-hint is-error" role="alert">{api.error}</p>}
          </main>
        </>
      )}
      {invite && detail && (
        <InviteSheet spaceName={detail.name} invite={invite} nowMs={now}
          onCancelInvite={() => { void api.cancelInvite(invite.space, invite.id); setInvite(null); }}
          onClose={() => setInvite(null)} />
      )}
      {(joiningOpen || (joining && joining.state !== 'joined')) && (
        <JoinSheet joining={joining} error={api.error}
          onJoin={(text, words) => void api.join(text, words)}
          onCancel={(space) => { if (space) void api.cancelJoin(space); setJoiningOpen(false); }}
          onOpenSpace={(space) => { setSelected(space); setJoiningOpen(false); }} />
      )}
      <JoinRequestSheet request={request} onAnswer={(s, r, allow) => void api.answer(s, r, allow)} />
      {creating && null /* D4.5 mounts NewSpaceSheet here */}
    </div>
  );
}
```
Remove the `creating && null` placeholder line in D4.5 when `NewSpaceSheet` is mounted. Until then `New space`
does nothing visible; D4.5's page test proves it.

### Tests to write (`SpacesPage.test.tsx`, mock `lib/spaces` hooks and `invoke`)
| Test | Input | Expected |
|---|---|---|
| `empty state explains spaces` | view with 0 spaces | heading `Spaces`; text `Chat with friends in small groups, computer to computer. No server keeps your messages.`; buttons `New space`, `Join a space` |
| `rail lists spaces with unread and online` | 2 spaces, unread 3, online 2 / 0 | `Book club`, badge `3`, `2 online`, `No one online` |
| `first space is selected by default` | 2 spaces | first `aria-current="page"` |
| `offline banner` | `online: false` | `You're offline. New messages will go out when you're back online.` |
| `owner sees invite button and the sheet opens` | detail `isOwner: true`; `invite` resolves | click `Invite people` -> `Invite someone to Book club` |
| `member does not see invite button` | `isOwner: false` | no `Invite people` |
| `join request sheet shows when the backend pushes one` | view.requests has Sam | `Sam wants to join Book club` |
| `join sheet opens from the rail` | click `Join a space` | heading `Join a space` |
| `sidebar shows Spaces row only when Labs is on` | render `Sidebar` with `spaces={{enabled:false}}` and `{enabled:true}` | row absent / present with label `Spaces` |

### Run
```bash
npx vitest run packages/app/src/components/spaces packages/app/src/components/cockpit
# expected: no failed tests; SpacesPage.test.tsx: 9 passed
```

### Done when
- [ ] With `spaces_enabled` off, no Spaces row and the page cannot be reached (same as desk, facts §11).
- [ ] 9 page tests pass; existing Sidebar/Cockpit tests pass.
- [ ] Screenshot not needed (R1); the page-level test is the proof.

### Do NOT
- R1-R7. Never start the app to look at the page; tests only.
- Do not add Spaces to the phone/PWA UI (`PwaApp.tsx`, `pwa/`; Spaces guard).
- Do not change the desk row.

### If it fails
1. `MainView` is a union in another file: add `| 'spaces'` there (allowed exception).
2. Sidebar test cannot find the row: copy the exact element and `role` the desk row uses.
3. `useSpace` with `null` calls invoke: it must return early when `space` is null.

---

## D4.3 `ChannelView`, `MessageRow`, `Composer`
Needs merged first: D4.2.

### Goal
Add the channel feed (messages, older pages, reply previews, deleted and blocked rows) and the composer (Enter to
send, Shift+Enter new line, 4000-char limit, reply bar).

### Read first
- `lib/spaces.ts` (`useChannel`, `SpaceMessage`), `components/spaces/SpacesPage.tsx`, `styles/cockpit-spaces.css`.

### Files
- create `packages/app/src/components/spaces/ChannelView.tsx`
- create `packages/app/src/components/spaces/MessageRow.tsx`
- create `packages/app/src/components/spaces/Composer.tsx`
- create `packages/app/src/components/spaces/ChannelView.test.tsx`
- create `packages/app/src/components/spaces/Composer.test.tsx`

### Steps
1. Create the three components (below). `ChannelView` uses `useChannel`.
2. Tests. Not mounted yet; D4.4 mounts `ChannelView` in `SpacesPage`.

### Code
`MessageRow.tsx`:
```tsx
import type { SpaceMessage } from '../../lib/spaces';

export const REACTION_PICTURE: Record<string, string> = {
  thumbs_up: '\u{1F44D}', heart: '\u{2764}\u{FE0F}', laugh: '\u{1F602}', wow: '\u{1F62E}',
  sad: '\u{1F622}', party: '\u{1F389}', eyes: '\u{1F440}', check: '\u{2705}',
};

export function timeOf(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

export interface MessageRowProps {
  message: SpaceMessage;
  onReply: (m: SpaceMessage) => void;
  /** D5.3 passes the moderation menu; until then nothing. */
  menu?: (m: SpaceMessage) => JSX.Element | null;
  onReact?: (m: SpaceMessage, code: string, on: boolean) => void;
  revealBlocked?: boolean;
  onRevealBlocked?: () => void;
}

export function MessageRow({ message: m, onReply, menu, onReact, revealBlocked, onRevealBlocked }: MessageRowProps) {
  const cls = ['ck-spaces-msg', m.isMine && 'is-mine', m.isFish && 'is-fish', m.deleted && 'is-deleted',
    m.blocked && !revealBlocked && 'is-blocked'].filter(Boolean).join(' ');
  if (m.blocked && !revealBlocked) {
    return (
      <li className={cls}>
        <span className="ck-spaces-msg-text">Message from someone you blocked.</span>
        <button type="button" className="ck-spaces-link" onClick={onRevealBlocked}>Show</button>
      </li>
    );
  }
  return (
    <li className={cls} data-id={m.id}>
      <div className="ck-spaces-msg-line">
        <span className="ck-spaces-msg-author">{m.authorName}</span>
        <time className="ck-spaces-msg-time" dateTime={new Date(m.createdAtMs).toISOString()}>{timeOf(m.createdAtMs)}</time>
        {!m.deleted && (
          <button type="button" className="ck-spaces-link" onClick={() => onReply(m)}>Reply</button>
        )}
        {menu?.(m)}
      </div>
      {m.replyPreview && (
        <p className="ck-spaces-msg-reply">{m.replyPreview.authorName}: {m.replyPreview.text}</p>
      )}
      {m.deleted ? (
        <p className="ck-spaces-msg-text">
          {m.isMine && !m.deleted.byOwner ? 'You deleted this message.'
            : m.deleted.byOwner ? 'Deleted by the owner.' : `${m.deleted.byName} deleted this message.`}
        </p>
      ) : (
        <p className="ck-spaces-msg-text">{m.text}</p>
      )}
      {m.reactions.length > 0 && !m.deleted && (
        <ul className="ck-spaces-reactions">
          {m.reactions.map((r) => (
            <li key={r.code}>
              <button type="button" className={`ck-spaces-reaction${r.mine ? ' is-mine' : ''}`}
                aria-pressed={r.mine} aria-label={`${r.code.replace('_', ' ')}, ${r.count}`}
                onClick={() => onReact?.(m, r.code, !r.mine)}>
                {REACTION_PICTURE[r.code]} {r.count}
              </button>
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}
```
(The pictures are written as `\u{...}` escapes on purpose: the source stays ASCII.)

`Composer.tsx`:
```tsx
import { useState, type KeyboardEvent } from 'react';
import type { SpaceMessage } from '../../lib/spaces';

export const MAX_CHARS = 4000;

export interface ComposerProps {
  channelName: string;
  replyTo: SpaceMessage | null;
  onCancelReply: () => void;
  onSend: (text: string) => Promise<string | null>;  // returns an error text or null
  disabled?: boolean;
}

export function Composer({ channelName, replyTo, onCancelReply, onSend, disabled }: ComposerProps) {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const count = [...text].length;
  const tooLong = count > MAX_CHARS;
  const send = async () => {
    if (busy || tooLong || text.trim() === '') return;
    setBusy(true);
    const err = await onSend(text);
    setBusy(false);
    if (err) { setError(err); return; }
    setError(null); setText(''); onCancelReply();
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void send(); }
  };
  return (
    <div className="ck-spaces-composer">
      {replyTo && (
        <div className="ck-spaces-composer-reply">
          <span>Replying to {replyTo.authorName}</span>
          <button type="button" className="ck-spaces-link" onClick={onCancelReply}>Cancel reply</button>
        </div>
      )}
      <textarea aria-label={`Message #${channelName}`} placeholder={`Message #${channelName}`} rows={1}
        value={text} disabled={disabled || busy} onChange={(e) => setText(e.target.value)} onKeyDown={onKey} />
      {count > 3500 && <span className={`ck-spaces-count${tooLong ? ' is-error' : ''}`}>{count} / 4000</span>}
      {tooLong && <p className="ck-spaces-hint is-error">This message is too long. The limit is 4000 characters.</p>}
      {error && <p className="ck-spaces-hint is-error" role="alert">{error}</p>}
      <button type="button" className="ck-spaces-btn is-primary" disabled={disabled || busy || tooLong || text.trim() === ''}
        onClick={() => void send()}>Send</button>
    </div>
  );
}
```

`ChannelView.tsx`:
```tsx
import { useEffect, useRef, useState } from 'react';
import { useChannel, type SpaceChannel, type SpaceMessage } from '../../lib/spaces';
import { MessageRow } from './MessageRow';
import { Composer } from './Composer';

export interface ChannelViewProps {
  space: string;
  channel: SpaceChannel;
  /** D5.3 passes these. */
  menu?: (m: SpaceMessage) => JSX.Element | null;
  onReact?: (m: SpaceMessage, code: string, on: boolean) => void;
}

export function ChannelView({ space, channel, menu, onReact }: ChannelViewProps) {
  const ch = useChannel(space, channel.id);
  const [replyTo, setReplyTo] = useState<SpaceMessage | null>(null);
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const end = useRef<HTMLDivElement>(null);
  const lastId = ch.messages[ch.messages.length - 1]?.id;
  useEffect(() => { end.current?.scrollIntoView?.({ block: 'end' }); }, [lastId]);
  return (
    <section className="ck-spaces-feed" aria-label={`#${channel.name}`}>
      <h2 className="ck-spaces-feed-title"># {channel.name}</h2>
      <ol className="ck-spaces-msgs">
        {ch.atStart ? (
          <li className="ck-spaces-start">This is the start of #{channel.name}.</li>
        ) : (
          <li><button type="button" className="ck-spaces-link" onClick={() => void ch.loadOlder()}>Show older messages</button></li>
        )}
        {ch.messages.map((m) => (
          <MessageRow key={m.id} message={m} onReply={setReplyTo} menu={menu} onReact={onReact}
            revealBlocked={revealed.has(m.id)} onRevealBlocked={() => setRevealed(new Set(revealed).add(m.id))} />
        ))}
      </ol>
      <div ref={end} />
      {ch.error && <p className="ck-spaces-hint is-error" role="alert">{ch.error}</p>}
      <Composer channelName={channel.name} replyTo={replyTo} onCancelReply={() => setReplyTo(null)}
        disabled={channel.archived} onSend={(text) => ch.send(text, replyTo?.id ?? null)} />
    </section>
  );
}
```

### Tests to write
`ChannelView.test.tsx` (mock `useChannel`):
| Test | Input | Expected |
|---|---|---|
| `shows messages oldest first with author and time` | 2 messages (Sam 10:02 "hi", Ana 10:03 "yo") | order hi, yo; `Sam`, `10:02` |
| `start of channel line` | `atStart: true` | `This is the start of #general.` |
| `older button loads older` | `atStart: false`, click `Show older messages` | `loadOlder` called once |
| `fish rows are marked` | `isFish: true`, author `Fish (Ren's)` | row has class `is-fish`, text `Fish (Ren's)` |
| `deleted row texts` | deleted by self / by owner / by Sam | `You deleted this message.` / `Deleted by the owner.` / `Sam deleted this message.` |
| `blocked row hides text until Show` | `blocked: true` text "secret" | `Message from someone you blocked.`; no `secret`; click `Show` -> `secret` |
| `reply sets the reply bar` | click `Reply` on Sam's row | `Replying to Sam` |
| `text is not rendered as html` | text `<b>x</b>` | the literal `<b>x</b>` is shown; no `b` element |
| `archived channel disables composer` | `channel.archived: true` | textarea disabled |

`Composer.test.tsx`:
| Test | Input | Expected |
|---|---|---|
| `enter sends and clears` | type "hi", Enter | `onSend("hi")`; textarea empty |
| `shift enter adds a new line` | Shift+Enter | `onSend` not called; value contains `\n` |
| `empty text cannot be sent` | spaces only | Send disabled; Enter does nothing |
| `counter shows above 3500` | 3600 chars | `3600 / 4000` |
| `too long blocks send` | 4001 chars | `This message is too long. The limit is 4000 characters.`; Send disabled |
| `server error is shown and text kept` | `onSend` returns `You're sending messages too fast. Wait a few seconds.` | that text in `alert`; textarea still has the text |
| `cancel reply` | `replyTo` set, click `Cancel reply` | `onCancelReply` called |

### Run
```bash
npx vitest run packages/app/src/components/spaces/ChannelView.test.tsx packages/app/src/components/spaces/Composer.test.tsx
# expected: Tests  16 passed (16)
```

### Done when
- [ ] 16 tests pass. Not mounted until D4.4.

### Do NOT
- R1-R7.
- No `dangerouslySetInnerHTML` anywhere in `components/spaces/`.
- No emoji characters typed literally in source; use the `\u{...}` escapes above.
- No colour classes; `is-fish` is styled with an outline or italic only (monochrome).

### If it fails
1. `scrollIntoView` is not a function in jsdom: the optional call `?.(...)` handles it; keep it.
2. IME composition sends early: keep the `isComposing` check.
3. Counter counts UTF-16 units: use `[...text].length` (code points) as written; the backend counts chars too.

---

## D4.4 `ChannelList`, `NewChannelSheet`, mount the feed
Needs merged first: D4.3.

### Goal
Add the channel column (channels with unread counts, `New channel` for the owner) and the new-channel sheet, and
mount `ChannelList` + `ChannelView` in `SpacesPage`.

### Read first
- `components/spaces/SpacesPage.tsx`, `ChannelView.tsx`, `lib/spaces.ts` (`spaceChannelCreate`).

### Files
- create `packages/app/src/components/spaces/ChannelList.tsx`
- create `packages/app/src/components/spaces/NewChannelSheet.tsx`
- create `packages/app/src/components/spaces/ChannelList.test.tsx`
- edit `packages/app/src/components/spaces/SpacesPage.tsx`
- edit `packages/app/src/components/spaces/SpacesPage.test.tsx`

### Steps
1. Create `ChannelList.tsx` and `NewChannelSheet.tsx` (below).
2. `SpacesPage.tsx`: state `channel` (id); when `detail` changes and `channel` is null or not in active channels, set
   it to the first non-archived channel; render under the header: `<div className="ck-spaces-body"><ChannelList ... />
   {current && <ChannelView space={detail.id} channel={current} />}</div>`; mount `NewChannelSheet` when
   `addingChannel`.
3. Tests.

### Code
`ChannelList.tsx`:
```tsx
import type { SpaceChannel } from '../../lib/spaces';

export interface ChannelListProps {
  channels: SpaceChannel[];
  selected: string | null;
  isOwner: boolean;
  onSelect: (id: string) => void;
  onNew: () => void;
}

export function ChannelList({ channels, selected, isOwner, onSelect, onNew }: ChannelListProps) {
  const active = channels.filter((c) => !c.archived);
  return (
    <nav className="ck-spaces-channels" aria-label="Channels">
      <ul>
        {active.map((c) => (
          <li key={c.id}>
            <button type="button"
              className={`ck-spaces-channel${c.id === selected ? ' is-active' : ''}${c.unread > 0 ? ' is-unread' : ''}`}
              aria-current={c.id === selected ? 'page' : undefined} onClick={() => onSelect(c.id)}>
              <span># {c.name}</span>
              {c.unread > 0 && <span className="ck-spaces-badge" aria-label={`${c.unread} unread`}>{c.unread}</span>}
            </button>
          </li>
        ))}
      </ul>
      {isOwner && (
        <button type="button" className="ck-spaces-link" disabled={active.length >= 20}
          title={active.length >= 20 ? 'This space has 20 channels. Archive one first.' : undefined}
          onClick={onNew}>New channel</button>
      )}
    </nav>
  );
}
```

`NewChannelSheet.tsx`:
```tsx
import { useId, useState } from 'react';
import { Sheet } from '../cockpit/Sheet';

export interface NewChannelSheetProps {
  onCreate: (name: string) => Promise<string | null>;   // error text or null
  onClose: () => void;
}

/** Same rule as Rust clean_channel_name, for the live preview only. */
export function previewChannelName(raw: string): string {
  return raw.trim().toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '').replace(/^-+|-+$/g, '').slice(0, 32);
}

export function NewChannelSheet({ onCreate, onClose }: NewChannelSheetProps) {
  const titleId = useId();
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const preview = previewChannelName(name);
  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={onClose} className="ck-spaces-sheet">
      <h2 id={titleId} className="ck-spaces-sheet-title">New channel</h2>
      <label className="ck-spaces-field">
        <span>Name</span>
        <input value={name} placeholder="books" onChange={(e) => setName(e.target.value)} />
        <small className="ck-spaces-hint">Lowercase letters, numbers and dashes.</small>
      </label>
      {preview !== '' && <p className="ck-spaces-note"># {preview}</p>}
      {error && <p className="ck-spaces-hint is-error" role="alert">{error}</p>}
      <div className="ck-spaces-actions">
        <button type="button" className="ck-spaces-btn" onClick={onClose}>Cancel</button>
        <button type="button" className="ck-spaces-btn is-primary" disabled={preview === ''}
          onClick={async () => { const e = await onCreate(name); if (e) setError(e); else onClose(); }}>Create</button>
      </div>
    </Sheet>
  );
}
```

### Tests to write
`ChannelList.test.tsx`:
| Test | Input | Expected |
|---|---|---|
| `lists active channels with unread` | general (3 unread), books, old (archived) | `# general` badge `3`, `# books`; no `# old` |
| `owner sees New channel` | `isOwner: true` / `false` | present / absent |
| `twenty channels disables New channel` | 20 active | button disabled, title `This space has 20 channels. Archive one first.` |
| `previewChannelName matches the Rust rule` | `"  Book Club! "`, `"---"` | `book-club`, `""` |
| `new channel sheet shows server error` | `onCreate` returns `There is already a channel called #books.` | that text shown, sheet stays |

`SpacesPage.test.tsx` additions:
| Test | Input | Expected |
|---|---|---|
| `first channel opens with the feed` | detail with `general`, `books` | `# general` heading in the feed region |
| `clicking a channel switches the feed` | click `# books` | feed heading `# books` |

### Run
```bash
npx vitest run packages/app/src/components/spaces
# expected: no failed tests (ChannelList.test.tsx 5 passed; SpacesPage.test.tsx 11 passed)
```

### Done when
- [ ] Owner can open the New channel sheet; the feed shows on the page (page test).

### Do NOT
- R1-R7.
- Do not decide channel name rules in TS; the preview is only a hint, the Rust rule decides.

### If it fails
1. Feed does not render in the page test: mock `useChannel` in the page test too.
2. The selected channel disappears after archive: the effect must reselect the first active channel.
3. Disabled button with `title` not found: query by role `button` name `New channel`.

---

## D4.5 `MemberList`, `PresenceDot`, `NewSpaceSheet`, space info
Needs merged first: D4.4.

### Goal
Add the people column with presence dots and roles, the New space sheet, and the Space info sheet with the honest
storage note.

### Read first
- `components/spaces/SpacesPage.tsx`, `lib/spaces.ts`, `styles/cockpit-spaces.css`.

### Files
- create `packages/app/src/components/spaces/MemberList.tsx`
- create `packages/app/src/components/spaces/NewSpaceSheet.tsx`
- create `packages/app/src/components/spaces/MemberList.test.tsx`
- edit `packages/app/src/components/spaces/SpacesPage.tsx`
- edit `packages/app/src/components/spaces/SpacesPage.test.tsx`

### Steps
1. Create `MemberList.tsx` (with `PresenceDot` exported from the same file) and `NewSpaceSheet.tsx`.
2. `SpacesPage.tsx`: render `<MemberList members={detail.members} />` as the right column (class
   `ck-spaces-people`); on narrow screens a `People` button toggles it (CSS hides the column under 900 px, the button
   adds `is-open`). Replace the `creating && null` placeholder with `<NewSpaceSheet ... />` using `api.create`, then
   select the new space. Add a `Space info` button in the header that opens a `Sheet` with the space name, member
   count, and the note text from "Words on screen".
3. Tests.

### Code
`MemberList.tsx`:
```tsx
import type { PresenceState, SpaceMember } from '../../lib/spaces';

const LABEL: Record<PresenceState, string> = { online: 'Online', away: 'Away', offline: 'Offline' };

export function PresenceDot({ state }: { state: PresenceState }) {
  return <span className={`ck-spaces-dot is-${state}`} role="img" aria-label={LABEL[state]} title={LABEL[state]} />;
}

const ORDER: Record<PresenceState, number> = { online: 0, away: 1, offline: 2 };

export function sortMembers(list: SpaceMember[]): SpaceMember[] {
  return [...list].sort((a, b) =>
    (a.role === 'owner' ? -1 : 0) - (b.role === 'owner' ? -1 : 0)
    || ORDER[a.presence] - ORDER[b.presence]
    || a.name.localeCompare(b.name));
}

export interface MemberListProps {
  members: SpaceMember[];
  /** D5 passes the per-member menu (Remove, Block). */
  menu?: (m: SpaceMember) => JSX.Element | null;
}

export function MemberList({ members, menu }: MemberListProps) {
  const sorted = sortMembers(members);
  return (
    <aside className="ck-spaces-members" aria-label="People">
      <h2 className="ck-spaces-feed-title">People ({members.length})</h2>
      <ul>
        {sorted.map((m) => (
          <li key={m.member} className={`ck-spaces-member${m.blocked ? ' is-blocked' : ''}`}>
            <PresenceDot state={m.presence} />
            <span className="ck-spaces-member-name">{m.name}{m.isMe ? ' (you)' : ''}</span>
            {m.role === 'owner' && <span className="ck-spaces-role">Owner</span>}
            {m.blocked && <span className="ck-spaces-role">Blocked</span>}
            {menu?.(m)}
          </li>
        ))}
      </ul>
    </aside>
  );
}
```

`NewSpaceSheet.tsx`:
```tsx
import { useId, useState } from 'react';
import { Sheet } from '../cockpit/Sheet';

export interface NewSpaceSheetProps {
  onCreate: (name: string) => Promise<string | null>;   // returns the new space id, or null on error
  error: string | null;
  onClose: () => void;
}

export function NewSpaceSheet({ onCreate, error, onClose }: NewSpaceSheetProps) {
  const titleId = useId();
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={onClose} className="ck-spaces-sheet">
      <h2 id={titleId} className="ck-spaces-sheet-title">New space</h2>
      <label className="ck-spaces-field">
        <span>Name</span>
        <input value={name} maxLength={40} placeholder="Book club" onChange={(e) => setName(e.target.value)} />
        <small className="ck-spaces-hint">You can change it later.</small>
      </label>
      <p className="ck-spaces-note">It starts with one channel, #general. You are the owner.</p>
      {error && <p className="ck-spaces-hint is-error" role="alert">{error}</p>}
      <div className="ck-spaces-actions">
        <button type="button" className="ck-spaces-btn" onClick={onClose}>Cancel</button>
        <button type="button" className="ck-spaces-btn is-primary" disabled={busy || name.trim() === ''}
          onClick={async () => { setBusy(true); const id = await onCreate(name.trim()); setBusy(false); if (id) onClose(); }}>
          Create
        </button>
      </div>
    </Sheet>
  );
}
```

### CSS (whole `packages/app/src/styles/cockpit-spaces.css` after D4.5)
Token names: replace each `var(--sp-...)` below with the exact variable `cockpit-desk.css` uses for the same job
(Open facts U-1): `--sp-fg` = body text colour, `--sp-muted` = secondary text, `--sp-line` = borders, `--sp-surface`
= panel background, `--sp-hover` = hover background, `--sp-strong` = primary button background, `--sp-on-strong` =
text on the primary button, `--sp-radius` = corner radius, `--sp-font-small` = small text size.
```css
.ck-spaces { display: grid; grid-template-columns: 200px 1fr; height: 100%; color: var(--sp-fg); }
.ck-spaces-rail { border-right: 1px solid var(--sp-line); display: flex; flex-direction: column; overflow: auto; }
.ck-spaces-rail ul, .ck-spaces-channels ul, .ck-spaces-members ul, .ck-spaces-msgs, .ck-spaces-reactions,
.ck-spaces-words { list-style: none; margin: 0; padding: 0; }
.ck-spaces-rail-item { display: grid; grid-template-columns: 1fr auto; width: 100%; padding: 6px 10px; text-align: left;
  background: none; border: 0; border-radius: var(--sp-radius); color: inherit; cursor: pointer; }
.ck-spaces-rail-item:hover { background: var(--sp-hover); }
.ck-spaces-rail-item.is-active { background: var(--sp-hover); font-weight: 600; }
.ck-spaces-rail-sub { grid-column: 1 / -1; color: var(--sp-muted); font-size: var(--sp-font-small); }
.ck-spaces-rail-actions { margin-top: auto; display: flex; flex-direction: column; gap: 4px; padding: 8px; }
.ck-spaces-badge { min-width: 18px; padding: 0 5px; border-radius: 9px; background: var(--sp-strong);
  color: var(--sp-on-strong); font-size: var(--sp-font-small); text-align: center; }
.ck-spaces-main { display: flex; flex-direction: column; min-width: 0; }
.ck-spaces-head { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid var(--sp-line); }
.ck-spaces-title { font-size: 15px; font-weight: 600; margin: 0; }
.ck-spaces-status { color: var(--sp-muted); font-size: var(--sp-font-small); }
.ck-spaces-banner { margin: 0; padding: 6px 12px; border-bottom: 1px solid var(--sp-line); color: var(--sp-muted); }
.ck-spaces-body { display: grid; grid-template-columns: 180px 1fr 200px; flex: 1; min-height: 0; }
.ck-spaces-channels { border-right: 1px solid var(--sp-line); padding: 8px; overflow: auto; }
.ck-spaces-channel { display: flex; justify-content: space-between; width: 100%; padding: 4px 8px; background: none;
  border: 0; border-radius: var(--sp-radius); color: var(--sp-muted); cursor: pointer; }
.ck-spaces-channel.is-unread { color: var(--sp-fg); font-weight: 600; }
.ck-spaces-channel.is-active { background: var(--sp-hover); color: var(--sp-fg); }
.ck-spaces-feed { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
.ck-spaces-feed-title { font-size: 13px; font-weight: 600; margin: 8px 12px; }
.ck-spaces-msgs { flex: 1; overflow: auto; padding: 0 12px; }
.ck-spaces-msg { padding: 4px 0; }
.ck-spaces-msg-line { display: flex; align-items: baseline; gap: 8px; }
.ck-spaces-msg-author { font-weight: 600; }
.ck-spaces-msg-time { color: var(--sp-muted); font-size: var(--sp-font-small); }
.ck-spaces-msg-text { margin: 2px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; }
.ck-spaces-msg-reply { margin: 2px 0 0; padding-left: 8px; border-left: 2px solid var(--sp-line); color: var(--sp-muted);
  font-size: var(--sp-font-small); }
.ck-spaces-msg.is-fish .ck-spaces-msg-author { font-style: italic; }
.ck-spaces-msg.is-deleted .ck-spaces-msg-text, .ck-spaces-msg.is-blocked .ck-spaces-msg-text { color: var(--sp-muted);
  font-style: italic; }
.ck-spaces-reactions { display: flex; gap: 4px; margin-top: 4px; }
.ck-spaces-reaction { padding: 0 6px; border: 1px solid var(--sp-line); border-radius: 10px; background: none;
  color: inherit; cursor: pointer; }
.ck-spaces-reaction.is-mine { border-color: var(--sp-fg); }
.ck-spaces-start { color: var(--sp-muted); padding: 8px 0; }
.ck-spaces-composer { display: grid; grid-template-columns: 1fr auto; gap: 6px; padding: 8px 12px;
  border-top: 1px solid var(--sp-line); }
.ck-spaces-composer textarea { resize: none; min-height: 32px; max-height: 160px; padding: 6px 8px;
  border: 1px solid var(--sp-line); border-radius: var(--sp-radius); background: var(--sp-surface); color: inherit; }
.ck-spaces-composer-reply { grid-column: 1 / -1; display: flex; gap: 8px; color: var(--sp-muted);
  font-size: var(--sp-font-small); }
.ck-spaces-count { align-self: center; color: var(--sp-muted); font-size: var(--sp-font-small); }
.ck-spaces-count.is-error { color: var(--sp-fg); font-weight: 600; }
.ck-spaces-members { border-left: 1px solid var(--sp-line); padding: 8px; overflow: auto; }
.ck-spaces-member { display: flex; align-items: center; gap: 6px; padding: 3px 4px; }
.ck-spaces-member.is-blocked .ck-spaces-member-name { color: var(--sp-muted); text-decoration: line-through; }
.ck-spaces-role { color: var(--sp-muted); font-size: var(--sp-font-small); }
.ck-spaces-dot { width: 8px; height: 8px; border-radius: 50%; flex: none; border: 1.5px solid var(--sp-fg); }
.ck-spaces-dot.is-online { background: var(--sp-fg); }
.ck-spaces-dot.is-away { background: linear-gradient(90deg, var(--sp-fg) 50%, transparent 50%); }
.ck-spaces-dot.is-offline { background: transparent; border-color: var(--sp-muted); }
.ck-spaces-empty { margin: auto; max-width: 420px; text-align: center; color: var(--sp-muted); }
.ck-spaces-btn { padding: 4px 10px; border: 1px solid var(--sp-line); border-radius: var(--sp-radius);
  background: var(--sp-surface); color: inherit; cursor: pointer; }
.ck-spaces-btn:hover { background: var(--sp-hover); }
.ck-spaces-btn.is-primary { background: var(--sp-strong); color: var(--sp-on-strong); border-color: var(--sp-strong); }
.ck-spaces-btn.is-danger { font-weight: 600; }
.ck-spaces-btn:disabled { opacity: 0.5; cursor: default; }
.ck-spaces-link { background: none; border: 0; padding: 0; color: var(--sp-muted); cursor: pointer;
  font-size: var(--sp-font-small); text-decoration: underline; }
.ck-spaces-sheet { max-width: 440px; }
.ck-spaces-sheet-title { font-size: 15px; font-weight: 600; margin: 0 0 8px; }
.ck-spaces-sheet-body { margin: 0 0 12px; }
.ck-spaces-field { display: flex; flex-direction: column; gap: 4px; margin-bottom: 10px; }
.ck-spaces-field input, .ck-spaces-field textarea { padding: 6px 8px; border: 1px solid var(--sp-line);
  border-radius: var(--sp-radius); background: var(--sp-surface); color: inherit; font: inherit; }
.ck-spaces-words { display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; counter-reset: w; margin-bottom: 10px; }
.ck-spaces-word { counter-increment: w; padding: 4px 6px; border: 1px solid var(--sp-line); border-radius: var(--sp-radius);
  font-family: ui-monospace, monospace; }
.ck-spaces-word::before { content: counter(w) ". "; color: var(--sp-muted); }
.ck-spaces-qr { display: flex; flex-direction: column; align-items: center; gap: 6px; margin-bottom: 10px; }
.ck-spaces-note { margin: 0 0 8px; color: var(--sp-muted); }
.ck-spaces-hint { margin: 0 0 8px; color: var(--sp-muted); font-size: var(--sp-font-small); }
.ck-spaces-hint.is-error { color: var(--sp-fg); font-weight: 600; }
.ck-spaces-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
@media (max-width: 900px) {
  .ck-spaces-body { grid-template-columns: 160px 1fr; }
  .ck-spaces-members { display: none; }
  .ck-spaces-members.is-open { display: block; position: absolute; right: 0; top: 48px; bottom: 0; width: 220px;
    background: var(--sp-surface); }
}
```

### Tests to write
`MemberList.test.tsx`:
| Test | Input | Expected |
|---|---|---|
| `owner first then online then name` | Bo(offline), Ren(owner, offline), Ana(online) | order Ren, Ana, Bo |
| `presence dot labels` | online / away / offline | `aria-label` `Online` / `Away` / `Offline`; classes `is-online` / `is-away` / `is-offline` |
| `me is marked` | `isMe: true` name Ren | `Ren (you)` |
| `blocked member is marked` | `blocked: true` | label `Blocked`, class `is-blocked` |
| `count in heading` | 4 members | `People (4)` |
| `new space create disabled when empty` | name `"   "` | Create disabled |

`SpacesPage.test.tsx` additions:
| Test | Input | Expected |
|---|---|---|
| `new space sheet creates and selects` | click `New space`, type `Book club`, Create; `create` resolves `s9` | sheet closes; `s9` selected |
| `space info shows the honest storage note` | click `Space info` | `Messages live on members' computers. If everyone is offline, new messages wait. This computer keeps the last 90 days, up to 200 MB per space.` |

### Run
```bash
npx vitest run packages/app/src/components/spaces
# expected: no failed tests (MemberList.test.tsx 6 passed; SpacesPage.test.tsx 13 passed)
```

### Done when
- [ ] People column, New space and Space info work in tests; all `components/spaces` tests pass.

### Do NOT
- R1-R7.
- Do not use colour for presence (online/away/offline differ by fill only).
- Do not show member keys; only `short` ids (and only in the join request sheet).

### If it fails
1. `localeCompare` order differs by locale in CI: use `a.name.toLowerCase() < b.name.toLowerCase()` comparisons.
2. Narrow-screen CSS cannot be tested in jsdom: test only that the `People` button toggles `is-open`.
3. `create` returns null on error: `NewSpaceSheet` keeps open and shows `error` from `useSpaces`.

---

## Open facts

- NEEDS-FACT U-1: the CSS variable names used by `packages/app/src/styles/cockpit-desk.css` for text, muted text,
  borders, panel background, hover, primary button, radius and small font. Read that file and map them to the
  `--sp-*` placeholders in D4.5.
- NEEDS-FACT U-2: where `MainView` is declared and its current members. Search `type MainView` in
  `packages/app/src`.
- NEEDS-FACT U-3: how existing component tests mock `invoke` and `listen` from `@tauri-apps/api` (copy the setup).
  Read `packages/app/src/components/desk/DeskPage.test.tsx`.
- NEEDS-FACT U-4: whether `Sheet` forwards `className` to the panel or the backdrop (affects `.ck-spaces-sheet`
  `max-width`). Read `packages/app/src/components/cockpit/Sheet.tsx`.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| HTML/script injection in a message | Send `<img onerror=...>` as text | Plain-text rendering; no `dangerouslySetInnerHTML`; no Markdown | `text is not rendered as html` |
| Look-alike names | Two members named "Ren" | Owner label, `(you)` marker, short id in join sheet; names are cleaned of bidi tricks (D2.2) | `owner first then online then name`, D2.2 `clean_name_rules` |
| Fish impersonation | A member names themself "Fish (Ren's)" | Fish rows come only from kind 7 by the owner (D3.2) and get `is-fish`; a member's name in a normal row is not italic | `fish rows are marked`, D3.2 `fish_text_by_member_is_rejected` |
| Hidden text after block | Blocked member's text appears in previews | Blocked rows hide text until `Show` | `blocked row hides text until Show` |
| UI reachable from the phone or agents | PWA renders Spaces | Not added to PWA; Spaces guard | plan 21 `spaces_is_not_reachable_from_the_bridge_mcp_or_the_phone` |
| False sense of privacy | User thinks deleted = gone everywhere | Honest delete texts (D5.3) and storage note | `space info shows the honest storage note` |

## What cannot be verified without real hardware or a second computer

- Real message arrival into an open channel from another PC (the event path `spaces://message` end to end).
- Unread counts when two PCs read the same channel (read marks are per PC by design).
- How the layout looks on a real small laptop screen and with system font scaling (no screenshots on the primary
  screen, rule R1).
