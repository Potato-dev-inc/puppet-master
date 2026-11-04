# Spaces D-5: moderation and limits

Read `20-spaces-overview.md` and the shared rules R1-R7 at the top of
`21-spaces-d0-d1-transport-spike-and-secure-core.md`. R1-R7 apply in full to every slice here.

Writer's sources: `00-FORMAT.md` and `_facts/spaces-facts.md` only (facts cited as `facts §N`).

Slices: D5.1, D5.2, D5.3, D5.4, D5.5 (5 slices).

## Moderation rules (decided)

| Action | Who | What happens | Others see |
|---|---|---|---|
| Remove from space | Owner only | `RemoveMember{reason: removed}` + `Rotate` (new epoch key for everyone left), both signed and pushed at once; links to that person's computers close | Member list updates; no message in the feed |
| Leave | Any member except the owner | The member's PC sends a `Leave` entry, marks the space "left" (read-only) and stops syncing; when the owner's PC stores the `Leave`, it appends `RemoveMember{reason: left}` + `Rotate` without asking | Member list updates |
| Owner leaves | Not possible in v1 (plan decision P-10) | Button shows `You own this space. Owners can't leave in this version.` | |
| Block | Anyone, about anyone except themself | Local only, per space: their messages are hidden behind `Show`; Fish never answers them on my PC; their entries are still stored and forwarded (sync keeps working) | Nothing. They are not told. |
| Delete a message | Its author, or the owner for any message | A `Delete` entry (tombstone). Every PC that gets it hides the text and drops the sealed bytes | `Sam deleted this message.` / `Deleted by the owner.` |
| React | Any member | `React{code, on}` entry; last lamport wins per (message, member, code) | Counts under the message |
| Reply | Any member | `Text` entry with `ref` = the message | Quote line above the reply |
| Delete my copy | The person who was removed or who left | Deletes the space folder and forgets its keys on this PC | Nothing |

Removed or left person's own screen keeps what they already have, read-only (the keys for old epochs stay so they
can read it), until they click `Delete my copy`.

Honest limits (shown in UI text, never hidden): a modified app can ignore deletes; people who left can still read
new messages until the owner's computer is online to rotate the key.

## Words on screen (exact)

| Place | Text |
|---|---|
| Member menu | `Remove from space` (owner, on others), `Block`, `Unblock` |
| Remove sheet | title `Remove {name} from {space}?`; body `They will stop getting new messages. Messages they already have stay on their computer.`; buttons `Cancel`, `Remove` |
| Block sheet | title `Block {name}?`; body `You won't see their messages in {space}. They are not told. Fish won't answer them on your computer.`; buttons `Cancel`, `Block` |
| Message menu | `Reply`, `React`, `Delete` (own messages; owner sees it on all) |
| React picker labels | `Thumbs up`, `Heart`, `Laugh`, `Wow`, `Sad`, `Party`, `Eyes`, `Check` |
| Delete sheet | title `Delete this message?`; body `It disappears for everyone in this space. People who already saw it may still have a copy.`; buttons `Cancel`, `Delete` |
| Header button | `Leave space` (members), disabled with tooltip `You own this space. Owners can't leave in this version.` (owner) |
| Leave sheet | title `Leave {space}?`; body `You will stop getting new messages. Your copy stays on this computer until you delete it.`; buttons `Cancel`, `Leave` |
| Removed banner | `You were removed from {space}. You can still read what you have here.` + button `Delete my copy` |
| Left banner | `You left {space}.` + button `Delete my copy` |
| Leave pending line (owner offline) | `Until the owner's computer is online, people who left can still read new messages.` (in Space info) |
| Delete copy sheet | title `Delete your copy of {space}?`; body `This removes its messages and files from this computer. It can't be undone.`; buttons `Cancel`, `Delete my copy` |
| Too fast (send) | `You're sending messages too fast. Wait a few seconds.` |
| Too many this hour | `You've sent a lot of messages this hour. Try again later.` |
| Not allowed delete | `You can only delete your own messages.` |

---

## D5.1 Remove member with key rotation; leave; delete my copy
Needs merged first: D4.5 (plan 24).

### Goal
Owner removes a member (RemoveMember + Rotate in one step), members leave (Leave entry, owner auto-rotates), and a
removed/left person can delete their local copy.

### Read first
- `spaces/roster.rs` (apply rules 9-10, `grants_for_all`, `next_op`), `spaces/service.rs` (`send`, `push_control`,
  `refresh_permissions`, `run_sync`), `spaces/ingest.rs` (`apply_body` Leave branch), `team/keystore.rs`
  (`set_epoch_key`, `forget_team(team, last_epoch)`, facts §3).
- `team/membership.rs` `remove` and `rotate` signatures (facts §3) for naming only; do not call them.

### Files
- create `packages/app/src-tauri/src/spaces/moderation.rs`
- create `packages/app/src-tauri/src/spaces/moderation_tests.rs`
- edit `packages/app/src-tauri/src/spaces/service.rs`
- edit `packages/app/src-tauri/src/spaces/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`

### Steps
1. `moderation.rs`: pure `remove_ops` (below). `mod.rs` line is added in the same edit of `service.rs`'s module
   neighbour: add `pub mod moderation;` and `#[cfg(test)] mod moderation_tests;` to `spaces/mod.rs` (counts as part
   of this slice; `mod.rs` edits are always allowed as the module list).
2. `service.rs`: `remove_member`, `leave`, `forget`, and the owner auto-rotate on stored `Leave`; the "removed" and
   "left" states in `view()`/`detail()`.
3. `commands.rs` + `lib.rs`: `space_remove_member`, `space_leave`, `space_forget`.
4. Tests.

### Code
```rust
//! Moderation helpers. Pure: build the ops; the service applies, stores and pushes them.
use super::identity::SpaceIdentity;
use super::ids::MemberKey;
use super::roster::{ControlOp, OpBody, RemoveReason, Roster, RosterError};

/// RemoveMember + Rotate, ready to apply in this order. `new_key` is a fresh random 32-byte key.
pub fn remove_ops(roster: &Roster, owner: &SpaceIdentity, member: &MemberKey, reason: RemoveReason,
                  new_key: &[u8; 32], now_ms: u64) -> Result<[ControlOp; 2], RosterError> {
    let remove = roster.next_op(owner, OpBody::RemoveMember { member: *member, reason }, now_ms)?;
    let mut after = roster.clone();
    after.apply(&remove)?;
    let grants = after.grants_for_all(after.epoch + 1, new_key)?;
    let rotate = after.next_op(owner, OpBody::Rotate { epoch: after.epoch + 1, grants }, now_ms)?;
    Ok([remove, rotate])
}
```

Service (`impl SpacesService`):
```rust
pub async fn remove_member(&self, space: &str, member: &str) -> Result<(), String>;
// owner only ("Only the owner can remove people."); not self; member must exist ("That person isn't in this space.");
// new_key = random_bytes::<32>(); ops = remove_ops(..); keystore.set_epoch_key(tag, epoch+1, &new_key) FIRST;
// apply + control_append both in one store transaction; keys.0.insert(epoch+1, new_key);
// push_control both ops to every live sync stream (the removed person's too, so they learn it);
// then close links to every node of the removed member that is not a member of another shared space;
// refresh_permissions(); changed().

pub async fn leave(&self, space: &str) -> Result<(), String>;
// owner -> Err("You own this space. Owners can't leave in this version.");
// send(Body::Leave) on the first channel; meta role = "left"; after the Leave entry was written to at least one
// live link, or after 24 h, stop dialing for this space; changed().

pub fn forget(&self, space: &str) -> Result<(), String>;
// only when role is "left" or "removed" ("Leave the space first."); close the store; delete the space folder;
// keystore.forget_team(&space_tag(id), roster.epoch); forget every invite code; refresh_permissions(); changed().
```
Owner auto-rotate: in the ingest path of the service, when `ingest_entry` returns `Stored` for a `Leave` entry, and I
am the owner, and the author member is still in the roster: run the same steps as `remove_member` with
`RemoveReason::Left`, without a UI sheet.

Removed detection: after `ApplyControl` on a member's PC, if a `RemoveMember` names my member key: meta role =
`removed`; stop dialing for this space; `detail()` gets `"state": "removed"`.
`detail()` JSON gains `"state": "active" | "left" | "removed"` and `"leavePending": bool` (owner side: true when a
`Leave` entry is stored but no `RemoveMember` for that member exists yet — only possible for a short time).

Commands:
```rust
#[tauri::command] pub async fn space_remove_member(app: AppHandle, space: String, member: String) -> Result<Value, String>;
#[tauri::command] pub async fn space_leave(app: AppHandle, space: String) -> Result<Value, String>;
#[tauri::command] pub async fn space_forget(app: AppHandle, space: String) -> Result<Value, String>;
```

### Tests to write (`spaces/moderation_tests.rs`; services on `memnet`: A owner, B, C)
| Test | Input | Expected |
|---|---|---|
| `remove_ops_are_remove_then_rotate` | `remove_ops` for B | op[0] RemoveMember, op[1] Rotate epoch 2, both apply on a clone |
| `removed_member_cannot_open_new_epoch` | A removes B; A sends "after" | C reads "after"; B's store has the entry (if it got it) but no messages row; B cannot open epoch 2 |
| `removed_member_learns_it` | A removes B while B online | B's detail `state == "removed"`; B stops dialing (no new links from B within 5 s) |
| `removed_node_is_refused_after_remove` | B dials A after removal (no other shared space) | refused |
| `member_in_another_space_keeps_a_link` | B also in space S2 with A | link A-B stays; B gets no S1 sync (first frame Hello for S1 refused with Bye 3) |
| `leave_auto_rotates_on_owner` | B leaves; A online | A's roster: B gone, epoch 2; no UI request created |
| `owner_cannot_leave` | A `leave` | `Err("You own this space. Owners can't leave in this version.")` |
| `forget_needs_left_or_removed` | B `forget` while active | `Err("Leave the space first.")` |
| `forget_deletes_folder_and_keys` | B removed, `forget` | folder gone; `keystore.epoch_key(tag, 1) == None` |
| `only_owner_removes` | B tries to remove C | `Err("Only the owner can remove people.")` |
| `rotation_key_is_saved_before_ops` | inject a failing store append after `set_epoch_key` | A can still open epoch 2 entries after restart (key present); ops retried on next remove attempt |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d51 cargo test --lib spaces::moderation_tests
# expected: test result: ok. 11 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d51 cargo test --lib spaces
# expected: 0 failed
```

### Done when
- [ ] 11 tests pass; removal always rotates (no code path appends RemoveMember without Rotate right after).

### Do NOT
- R1-R7.
- Do not call `team::membership::remove`/`rotate` (team code, team transport).
- Do not delete the removed person's old epoch keys on their PC (they keep read access to what they had).
- Do not show a message in the feed about removals (privacy of the removed person).

### If it fails
1. `Rotate` fails with `grants do not match devices`: grants must be computed on the roster AFTER applying
   RemoveMember (as in `remove_ops`).
2. C never gets epoch 2: `push_control` must send to every live stream, and C's sync applies ops then opens its grant.
3. Removed B keeps reconnecting: B's dial loop must skip spaces with role `removed`/`left`.

---

## D5.2 Block (local only) and TS wrappers for D-5
Needs merged first: D5.1.

### Goal
Let a person block or unblock a member in a space on their own PC, mark blocked authors in message JSON, and add the
TS wrappers for remove, leave, forget, block, react and delete.

### Read first
- `spaces/store.rs` (`blocks` table), `spaces/service.rs` (`messages`, `detail`), `lib/spaces.ts`.

### Files
- edit `packages/app/src-tauri/src/spaces/service.rs`
- edit `packages/app/src-tauri/src/spaces/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- edit `packages/app/src/lib/spaces.ts`
- edit `packages/app/src/lib/spaces.test.ts`

### Steps
1. Store: use `INSERT OR REPLACE INTO blocks (member, since_ms)` / `DELETE FROM blocks WHERE member = ?` (add
   `block_set(member, on, now)`, `is_blocked(member)`, `blocked()` to `store.rs` through the service edit — store.rs
   is allowed as the 6th file only for these three functions; if you prefer 5 files, put the three SQL statements
   inside `service.rs` methods using `store.conn`).
2. Service `block(space, member, on)`: not myself (`You can't block yourself.`); member must be in the roster or
   `past`; set; `changed()`.
3. `messages()` sets `blocked: true` for authors in `blocks`; `detail()` sets `blocked` on members.
4. Command `space_block(space, member, on)`.
5. TS wrappers and tests.

### Code
```rust
#[tauri::command] pub async fn space_block(app: AppHandle, space: String, member: String, on: bool) -> Result<Value, String>;
```
TS:
```ts
export const spaceRemoveMember = (space: string, member: string) =>
  invoke<SpaceDetail>('space_remove_member', { space, member });
export const spaceLeave = (space: string) => invoke<SpaceDetail>('space_leave', { space });
export const spaceForget = (space: string) => invoke<SpacesView>('space_forget', { space });
export const spaceBlock = (space: string, member: string, on: boolean) =>
  invoke<SpaceDetail>('space_block', { space, member, on });
export const spaceReact = (space: string, target: string, code: ReactionCode, on: boolean) =>
  invoke<SpaceMessage>('space_react', { space, target, code, on });
export const spaceDelete = (space: string, target: string) => invoke<SpaceMessage>('space_delete', { space, target });

export const REACTION_LABEL: Record<ReactionCode, string> = {
  thumbs_up: 'Thumbs up', heart: 'Heart', laugh: 'Laugh', wow: 'Wow',
  sad: 'Sad', party: 'Party', eyes: 'Eyes', check: 'Check',
};
```
Add `state: 'active' | 'left' | 'removed'` and `leavePending: boolean` to `SpaceDetail`.

### Tests to write
Rust (add to `moderation_tests.rs`):
| Test | Input | Expected |
|---|---|---|
| `block_is_local_only` | A blocks B | A's messages JSON for B `blocked: true`; B's and C's stores have no `blocks` rows; no entry or op was sent (A's seq unchanged) |
| `blocked_entries_still_forward` | A blocks B; chain B-A-C (cut B-C); B sends | C receives it through A |
| `cannot_block_self` | A blocks A | `Err("You can't block yourself.")` |
| `unblock_shows_again` | block then unblock | `blocked: false` |
TS (`spaces.test.ts`):
| Test | Input | Expected |
|---|---|---|
| `wrappers call the right commands` | each wrapper | `invoke` called with the command names and args above |
| `reaction labels cover every code` | keys of `REACTION_LABEL` | the 8 codes |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d52 cargo test --lib spaces::moderation_tests
# expected: test result: ok. 15 passed; 0 failed
cd ../../..
npx vitest run packages/app/src/lib/spaces.test.ts
# expected: Tests  15 passed (15)
```

### Done when
- [ ] Block never leaves the PC (test `block_is_local_only`); 4 Rust + 2 TS tests added and passing.

### Do NOT
- R1-R7.
- Do not drop or refuse a blocked member's entries in sync (it would split the space's history).
- Do not tell the blocked person anything.

### If it fails
1. `blocked_entries_still_forward` fails: the block check must be only in the JSON views, not in `ingest`/`forward`.
2. Member key format: TS passes hex; parse with `MemberKey::from_hex`, error `That person isn't in this space.`
3. TS count differs: the D4.1 count was 13; plus 2 = 15.

---

## D5.3 React and delete (backend)
Needs merged first: D5.2.

### Goal
Add `space_react` and `space_delete`: build and send `React` and `Delete` entries with the author/owner rule, and
return the updated message.

### Read first
- `spaces/log.rs` (`Body::React`, `Body::Delete`, `REACTIONS`), `spaces/ingest.rs` (`apply_body`),
  `spaces/service.rs` (`send`, `messages`).

### Files
- edit `packages/app/src-tauri/src/spaces/service.rs`
- edit `packages/app/src-tauri/src/spaces/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- edit `packages/app/src-tauri/src/spaces/service_tests.rs`

### Steps
1. Service `react(space, target, code, on)`: `code` must be in `REACTIONS` (`That reaction isn't available.`);
   target must be a shown, not deleted message (`That message is gone.`); `send(.., Body::React{code, on},
   Some(target))` on the target's channel; return the message JSON.
2. Service `delete(space, target)`: target must exist; allowed if `target.member == me` or I am the owner, else
   `You can only delete your own messages.`; `send(.., Body::Delete, Some(target))`; return the message JSON
   (now deleted).
3. Commands `space_react(space, target, code, on)`, `space_delete(space, target)`; register them.
4. Tests.

### Code
```rust
#[tauri::command] pub async fn space_react(app: AppHandle, space: String, target: String, code: String, on: bool)
    -> Result<Value, String>;
#[tauri::command] pub async fn space_delete(app: AppHandle, space: String, target: String) -> Result<Value, String>;
```

### Tests to write (`service_tests.rs` additions)
| Test | Input | Expected |
|---|---|---|
| `reaction_reaches_others_with_count` | A and C react `heart` on B's message | B sees `heart` count 2 |
| `reaction_off_removes_mine` | A reacts on, then off | count 0 everywhere |
| `unknown_reaction_is_refused` | code `fire` | `Err("That reaction isn't available.")` |
| `author_deletes_for_everyone` | B deletes own message | A and C show it deleted by B, text `""` |
| `owner_deletes_anyones` | A deletes B's message | deleted, `byOwner: true` |
| `member_cannot_delete_others` | C deletes B's message | `Err("You can only delete your own messages.")`; nothing sent |
| `forged_delete_from_member_is_ignored` | C hand-builds a Delete for B's message and pushes it | A and B keep the message (ingest rule, D3.3) |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d53 cargo test --lib spaces::service_tests
# expected: test result: ok. 20 passed; 0 failed
```

### Done when
- [ ] 7 new tests pass (13 from D4.1 + 7 = 20).

### Do NOT
- R1-R7.
- Do not implement "edit message" (kind 2 is reserved).
- Do not allow reacting to deleted messages.

### If it fails
1. Count is 1 instead of 2: reactions are keyed by member; A and C are different members — check the test uses
   two services, not one.
2. Delete arrives before the message on C: covered by D3.3 `delete_before_target_applies_when_target_arrives`.
3. `byOwner` wrong: compare `deleted_by` with `roster.owner`.

---

## D5.4 Message and member menus, leave and removed states in the UI
Needs merged first: D5.3.

### Goal
Add the message menu (Reply, React, Delete with confirm), the member menu (Remove, Block/Unblock with confirm),
the Leave button, and the left/removed banners with `Delete my copy`.

### Read first
- `components/spaces/MessageRow.tsx`, `MemberList.tsx`, `ChannelView.tsx`, `SpacesPage.tsx`, `lib/spaces.ts`,
  `components/cockpit/Sheet.tsx`.

### Files
- create `packages/app/src/components/spaces/MessageMenu.tsx`
- create `packages/app/src/components/spaces/MemberMenu.tsx`
- create `packages/app/src/components/spaces/Menus.test.tsx` (one test file for both menus; deviation from "one test
  file per component" to stay within 5 files)
- edit `packages/app/src/components/spaces/SpacesPage.tsx`
- edit `packages/app/src/components/spaces/SpacesPage.test.tsx`

### Steps
1. `MessageMenu.tsx`: a `More` button (`aria-label="Message actions"`) opening a small list: `Reply`, `React` (opens
   8 picture buttons with the labels from `REACTION_LABEL`), `Delete` (only when `canDelete`). Delete opens the
   Delete sheet (texts in "Words on screen").
2. `MemberMenu.tsx`: `More` button (`aria-label="Actions for {name}"`): `Remove from space` (when `canRemove`),
   `Block` / `Unblock` (not on me). Each opens its sheet.
3. `SpacesPage.tsx`: pass `menu` and `onReact` to `ChannelView` and `menu` to `MemberList`; add `Leave space` button
   in the header with its sheet; when `detail.state` is `left` or `removed` show the banner, hide the composer
   (pass `disabled` through `ChannelView` → `Composer`), and show `Delete my copy` with its sheet.
4. Tests.

### Code
```tsx
// MessageMenu.tsx
import { useId, useState } from 'react';
import { Sheet } from '../cockpit/Sheet';
import { REACTION_LABEL, type ReactionCode, type SpaceMessage } from '../../lib/spaces';
import { REACTION_PICTURE } from './MessageRow';

export interface MessageMenuProps {
  message: SpaceMessage;
  canDelete: boolean;
  onReply: () => void;
  onReact: (code: ReactionCode) => void;
  onDelete: () => void;
}

export function MessageMenu({ message, canDelete, onReply, onReact, onDelete }: MessageMenuProps) {
  const [open, setOpen] = useState(false);
  const [picking, setPicking] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const titleId = useId();
  if (message.deleted) return null;
  return (
    <span className="ck-spaces-menu">
      <button type="button" className="ck-spaces-link" aria-label="Message actions" aria-expanded={open}
        onClick={() => setOpen(!open)}>More</button>
      {open && (
        <span role="menu" className="ck-spaces-menu-list">
          <button type="button" role="menuitem" onClick={() => { setOpen(false); onReply(); }}>Reply</button>
          <button type="button" role="menuitem" onClick={() => setPicking(!picking)}>React</button>
          {canDelete && <button type="button" role="menuitem" onClick={() => { setOpen(false); setConfirm(true); }}>Delete</button>}
          {picking && (Object.keys(REACTION_LABEL) as ReactionCode[]).map((c) => (
            <button key={c} type="button" aria-label={REACTION_LABEL[c]}
              onClick={() => { setOpen(false); setPicking(false); onReact(c); }}>{REACTION_PICTURE[c]}</button>
          ))}
        </span>
      )}
      {confirm && (
        <Sheet labelledBy={titleId} onClose={() => setConfirm(false)} className="ck-spaces-sheet">
          <h2 id={titleId} className="ck-spaces-sheet-title">Delete this message?</h2>
          <p className="ck-spaces-sheet-body">
            It disappears for everyone in this space. People who already saw it may still have a copy.
          </p>
          <div className="ck-spaces-actions">
            <button type="button" className="ck-spaces-btn" onClick={() => setConfirm(false)}>Cancel</button>
            <button type="button" className="ck-spaces-btn is-danger" onClick={() => { setConfirm(false); onDelete(); }}>Delete</button>
          </div>
        </Sheet>
      )}
    </span>
  );
}
```
`MemberMenu.tsx` follows the same shape with props
`{ member: SpaceMember; spaceName: string; canRemove: boolean; onRemove: () => void; onBlock: (on: boolean) => void }`
and the two sheets with the exact texts from "Words on screen" (`Remove {name} from {space}?` / `Block {name}?`).
Unblock needs no sheet. `canRemove = detail.isOwner && !member.isMe && member.role !== 'owner'`.
`canDelete = message.isMine || detail.isOwner`.

Add CSS to `cockpit-spaces.css` is not needed for this slice if `.ck-spaces-menu` and `.ck-spaces-menu-list` are
added in the same `SpacesPage` edit — they are not; add these two rules to `cockpit-spaces.css` (allowed small
edit; it is a stylesheet):
```css
.ck-spaces-menu { position: relative; }
.ck-spaces-menu-list { position: absolute; right: 0; top: 18px; z-index: 5; display: flex; flex-direction: column;
  min-width: 140px; padding: 4px; border: 1px solid var(--sp-line); border-radius: var(--sp-radius); background: var(--sp-surface); }
```

### Tests to write
`Menus.test.tsx`:
| Test | Input | Expected |
|---|---|---|
| `message menu has reply react and delete for own` | `canDelete: true` | items `Reply`, `React`, `Delete` |
| `no delete for others` | `canDelete: false` | no `Delete` |
| `delete asks first with the honest text` | click Delete | `Delete this message?` + `People who already saw it may still have a copy.`; `onDelete` only after `Delete` in the sheet |
| `react picker has 8 labelled choices` | click React | 8 buttons with labels `Thumbs up` ... `Check`; click `Heart` -> `onReact('heart')` |
| `deleted message has no menu` | `deleted` set | renders nothing |
| `member menu remove confirm` | `canRemove: true`, name Sam, space Book club | `Remove Sam from Book club?`; `Remove` calls `onRemove` |
| `block confirm says they are not told` | click Block | `They are not told.` in the sheet; confirm calls `onBlock(true)` |
| `unblock has no sheet` | `blocked: true`, click `Unblock` | `onBlock(false)` at once |
| `no actions besides reply react delete` | message menu items | exactly those three item names (cowork M2: no action buttons from member content) |

`SpacesPage.test.tsx` additions:
| Test | Input | Expected |
|---|---|---|
| `owner leave button is disabled with reason` | owner detail | `Leave space` disabled, title `You own this space. Owners can't leave in this version.` |
| `member leaves after confirm` | member detail; click `Leave space`, then `Leave` | `space_leave` invoked |
| `removed banner hides composer and offers delete` | `state: 'removed'` | `You were removed from Book club. You can still read what you have here.`; no textarea; `Delete my copy` sheet text `This removes its messages and files from this computer. It can't be undone.` |

### Run
```bash
npx vitest run packages/app/src/components/spaces
# expected: no failed tests (Menus.test.tsx 9 passed; SpacesPage.test.tsx 16 passed)
```

### Done when
- [ ] Every moderation action has a confirm sheet with the exact text (except Unblock and React).
- [ ] Menus wired in the page (page tests).

### Do NOT
- R1-R7.
- Do not add any message action that runs code, opens links, merges, approves, or calls an agent (cowork M2).
- Do not put colour on `is-danger`; it is bold only.

### If it fails
1. Menu stays open after an action: every item handler sets `open` false first.
2. Sheet focus trap blocks the test click: use `findByRole('button', { name: 'Delete' })` inside the sheet.
3. `ChannelView` has no `disabled` prop: add `readOnly?: boolean` to `ChannelViewProps` and pass it to `Composer`
   `disabled` (this is an edit to `ChannelView.tsx`, allowed as part of wiring; note it in the commit).

---

## D5.5 Rate limits, link limits, and the cowork M1-M6 checks
Needs merged first: D5.4.

### Goal
Add deterministic per-author rate limits on entries, per-link frame limits, a capped lockout map, and tests that
prove each cowork M1-M6 lesson holds in Spaces.

### Read first
- `spaces/limits.rs`, `spaces/ingest.rs`, `spaces/service.rs` (`send`, `run_sync`), `spaces/join.rs` (`Lockouts`),
  `spaces/guard_tests.rs`.
- Facts §8 (cowork M1-M6 summary). The review file itself is outside the repo
  (`D:/pm-night/SECURITY-REVIEW-COWORK.md`); read its M-sections if available.

### Files
- create `packages/app/src-tauri/src/spaces/ratelimit.rs`
- create `packages/app/src-tauri/src/spaces/ratelimit_tests.rs`
- edit `packages/app/src-tauri/src/spaces/ingest.rs`
- edit `packages/app/src-tauri/src/spaces/service.rs`
- edit `packages/app/src-tauri/src/spaces/guard_tests.rs`

### Steps
1. `ratelimit.rs`: `author_over_limit` (deterministic, from stored `created_ms`) and `LinkMeter` (frames per second
   per link).
2. `ingest.rs`: after `log::check` returns `Ok`, call `author_over_limit(store, device, created_ms)`; if over:
   `Ingest::Rejected("too fast")` (not stored, so not forwarded).
3. `service.rs`: in `send`, call the same check for my device with `now`; if over, return the user texts
   (`You're sending messages too fast. Wait a few seconds.` for the 10 s rule, `You've sent a lot of messages this
   hour. Try again later.` for the hour rule). In `run_sync`, feed every frame to a `LinkMeter`; when it says
   `Close`, write `Bye(4)` and close the stream. Cap `Lockouts` at 256 nodes (drop the oldest `until`).
4. `guard_tests.rs`: add `merge_green`, `team::actions`, `quote_untrusted`, `auto_approve` to the words that no
   `spaces/` file may contain (new test `spaces_has_no_action_route_from_member_content`).
5. `mod.rs`: `pub mod ratelimit; #[cfg(test)] mod ratelimit_tests;` (module-list edit, allowed).

### Code
```rust
//! Abuse limits. Entry limits use the author's own timestamps so every PC decides the same way.
use super::ids::DeviceKey;
use super::limits::{RATE_BURST, RATE_BURST_WINDOW_MS, RATE_HOUR};
use super::store::SpaceStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Over { No, Burst, Hour }

/// Counts this author's stored entries in (created_ms - window, created_ms]. Fish (kind 7) and Leave are counted too.
pub fn author_over_limit(s: &SpaceStore, device: &DeviceKey, created_ms: u64) -> rusqlite::Result<Over> {
    let count = |window: u64| -> rusqlite::Result<u32> {
        s.conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE device = ?1 AND gone = 0 AND created_ms > ?2 AND created_ms <= ?3",
            rusqlite::params![&device.0[..], created_ms.saturating_sub(window) as i64, created_ms as i64],
            |r| r.get::<_, u32>(0),
        )
    };
    if count(RATE_BURST_WINDOW_MS)? >= RATE_BURST { return Ok(Over::Burst); }
    if count(60 * 60 * 1000)? >= RATE_HOUR { return Ok(Over::Hour); }
    Ok(Over::No)
}

pub const LINK_FRAMES_PER_SEC: u32 = 200;
pub const LINK_OVER_SECONDS: u32 = 3;

/// Frames per second on one stream. Three seconds in a row over the cap = close.
pub struct LinkMeter { second: u64, count: u32, over_in_row: u32 }

#[derive(Debug, PartialEq, Eq)]
pub enum Meter { Ok, Close }

impl LinkMeter {
    pub fn new() -> Self { Self { second: 0, count: 0, over_in_row: 0 } }
    pub fn frame(&mut self, now_ms: u64) -> Meter {
        let s = now_ms / 1000;
        if s != self.second {
            if self.count > LINK_FRAMES_PER_SEC { self.over_in_row += 1; } else { self.over_in_row = 0; }
            self.second = s;
            self.count = 0;
        }
        self.count += 1;
        if self.over_in_row >= LINK_OVER_SECONDS { Meter::Close } else { Meter::Ok }
    }
}
```

Cowork M1-M6 in Spaces (what each lesson means here, and the test that holds it):

| Item | Cowork finding (facts §8) | Spaces rule | Test |
|---|---|---|---|
| M1 | Relay mailbox counter restarts after the mailbox empties; clients never see new mail | No server counter anywhere. Sync uses per-author seqs chosen by the author; floors only grow; the D9.1 mailbox serves the same per-author seqs | `catch_up_after_everything_was_pruned` |
| M2 | Merge-when-green acts on whatever a teammate's card names | No action from member content: messages offer only Reply / React / Delete; no Spaces file references merge/actions code | `spaces_has_no_action_route_from_member_content` (guard), D5.4 `no actions besides reply react delete` |
| M3 | Relay memory/CPU exhaustion before auth | Strangers are admitted only while an invite is open; frame length checked before allocation; lockout map capped at 256; 20 joins per minute | `lockout_map_is_capped_at_256`, D2.6 `oversized_length_is_refused_without_allocating`, D2.5 `stranger_is_refused_when_no_invite_is_open` |
| M4 | Every post wakes every socket which queries SQLite | An entry is forwarded once per live stream, only when newly stored; a stream that floods is closed | `duplicate_push_is_not_forwarded`, `flooding_stream_is_closed` |
| M5 | Per-IP limits break behind a proxy; per-invite counter never resets; limits in memory only; no quota | Limits key on the authenticated node id or author device, never an IP; invite strikes live in SQLite; entry limits are recomputed from stored data after restart; 200 MB per space | `strikes_survive_restart`, `rate_limit_survives_restart`, D3.6 `size_rule_trims_to_180_mb` |
| M6 | Untrusted names outside the fence in an agent prompt; auto-approve | Fish gets channel text only inside a fence (D6.2), runs with zero tools (D7.1) | plan 26 `fence_survives_fence_breaking_text`, `tool_call_in_social_reply_is_not_executed` |

Release gate: `DECISION-OPEN (default: Spaces stays Labs-only until cowork M1-M6 are fixed in the team relay and
these tests pass)` (plan decision P-15).

### Tests to write (`spaces/ratelimit_tests.rs` unless noted)
| Test | Input | Expected |
|---|---|---|
| `eleventh_entry_in_ten_seconds_is_dropped` | 10 entries by Sam at t..t+9 s, 11th at t+9.5 s | 11th `Rejected("too fast")`, not stored, not forwarded |
| `burst_window_slides` | 11th at t+10.1 s | `Stored` |
| `six_hundred_first_in_an_hour_is_dropped` | 600 spread over 59 min (≤ 10 per 10 s), 601st | `Rejected("too fast")` |
| `my_own_send_gets_the_plain_text` | service send 11 times fast | 11th `Err("You're sending messages too fast. Wait a few seconds.")` |
| `flooding_stream_is_closed` | 250 frames/s for 4 s on one stream (manual clock) | `LinkMeter` gives `Close` in second 4; the service writes `Bye(4)` |
| `lockout_map_is_capped_at_256` | 300 nodes strike | `Lockouts` holds 256 |
| `duplicate_push_is_not_forwarded` | B receives the same Push twice from A | B forwards to C once |
| `strikes_survive_restart` | 3 wrong words, restart the owner service, 2 more | invite `dead` |
| `rate_limit_survives_restart` | 10 entries, restart, 11th within the window | rejected |
| `catch_up_after_everything_was_pruned` | all of Sam's entries pruned on B (floor 40), Sam sends seq 41 | B stores 41 (no reset to 1, no stuck cursor) |
| `spaces_has_no_action_route_from_member_content` (in `guard_tests.rs`) | scan `spaces/*.rs` | none contains `merge_green`, `team::actions`, `quote_untrusted`, `auto_approve` |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d55 cargo test --lib spaces::ratelimit_tests
# expected: test result: ok. 10 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d55 cargo test --lib spaces::guard_tests
# expected: test result: ok. 8 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d55 cargo test --lib spaces
# expected: 0 failed
```

### Done when
- [ ] 10 rate-limit tests and 8 guard tests pass; the M1-M6 table rows each have a passing test (D6/D7 rows pass
      after plan 26).

### Do NOT
- R1-R7.
- Do not key any limit on an IP address or a relay address.
- Do not keep the only copy of a limit counter in memory when it decides access to an invite (strikes are in SQLite).
- Do not throttle reading (`Want` answers) below the 500-per-round rule; catch-up must stay possible.

### If it fails
1. Burst test off by one: the window is `(created - 10 000, created]` and the check runs BEFORE inserting the new
   entry; 10 stored + the new one = 11th rejected.
2. A peer's backdated flood passes the author rule: the `LinkMeter` is the second layer; check it is fed in
   `run_sync`.
3. `Lockouts` cap drops the wrong node: drop the entry with the smallest `until` (the oldest).

---

## Open facts

- NEEDS-FACT M-1: the exact text of cowork M1-M6 detail sections (only the summary is in facts §8). Read
  `D:/pm-night/SECURITY-REVIEW-COWORK.md` lines 51-170 before D5.5 and check each row of the table still matches.
- NEEDS-FACT M-2: whether `KeyStore::forget_team(team, last_epoch)` also removes invite codes for that team name, or
  each must be removed with `forget_invite`. Read `packages/app/src-tauri/src/team/keystore.rs` lines 116-135.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| Removed member keeps reading | Uses old key for new messages | Rotate with grants only to remaining devices, in the same step as remove | `removed_member_cannot_open_new_epoch` |
| Remove without rotate | A bug appends only RemoveMember | Roster rule `rotate first` blocks every other op; `remove_ops` builds both | D2.3 `remove_requires_rotate_next`, `remove_ops_are_remove_then_rotate` |
| Owner loses the new key mid-remove | Crash between ops and key save | Key saved to the keystore before ops are stored | `rotation_key_is_saved_before_ops` |
| Member deletes others' messages | Forged Delete | Ingest checks author or owner | `forged_delete_from_member_is_ignored` |
| Flood a channel | Many entries | 10 per 10 s and 600 per hour per device, decided the same on every PC; link meter closes floods | `eleventh_entry_in_ten_seconds_is_dropped`, `flooding_stream_is_closed` |
| Block leaks to the blocked person | Block sends something | Block is a local table only | `block_is_local_only` |
| Blocking breaks the space | Entries refused | Blocked entries are stored and forwarded | `blocked_entries_still_forward` |
| Restart resets limits (cowork M5) | Restart to get 5 more tries | Strikes in SQLite; entry limits from stored rows | `strikes_survive_restart`, `rate_limit_survives_restart` |
| Action buttons on member content (cowork M2) | A message that triggers a merge or a run | Only Reply/React/Delete exist; guard bans action code | `spaces_has_no_action_route_from_member_content` |
| A modified client ignores deletes | Keeps deleted text | Not preventable; told in the delete sheet | none (documented) |
| Left member reads until owner is online | Leave is honoured late | Told in Space info | none (documented) |

## What cannot be verified without real hardware or a second computer

- Removal and rotation when the removed person is offline for days and then comes back (they should get the ops and
  become read-only).
- Rate limits under real clock skew between PCs (the author-timestamp rule assumes clocks within 10 minutes).
- A real flood over a relayed link (the relay's own limits are not ours to test).
