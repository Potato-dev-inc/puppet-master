# My computers: the two-PC test (30 minutes, plus 1 hour idle)

You run this on two real Windows PCs on the same network. It is the gate before the Labs switch can come off
(plan DL-11, DevHop checks N6.1 to N6.5). Nothing in the automated tests can prove these things: the real pointer,
the real keys, the real clipboard and a real cable. Write down what you see in the sheet at the end. Short notes are
enough. "It worked" is a good note.

Words: **PC A** is the one you sit at. **PC B** is the one on its right.

## Before you start (3 minutes)

- Both PCs run the same Puppet Master build. Write the build id from About on each.
- Labs, My computers: on, on both.
- Same network, same subnet. Wired is best. Write down: wired or Wi-Fi, for each PC.
- Turn sleep off on both PCs for the test.
- Put the PCs side by side. PC B is on the right of PC A.
- Know the three ways out if the pointer gets lost:
  1. **Ctrl + Alt + Esc** (panic): the pointer comes home and every key is let go, on both PCs.
  2. **Scroll Lock** (pause), then **Resume sharing** on the page when you want it back.
  3. Quit Puppet Master. The pointer comes home when it quits.
- Windows Security (or your antivirus): write down if it warned about anything when you turned sharing on.

## 1. Pair, first try (5 minutes) — check N6.1

1. On PC A: My computers, **Add a computer**. It shows a code and an address.
2. On PC B: **Add a computer**, type the address and the code from PC A.
3. Both PCs ask "wants to pair" with the same code. Say yes on both.
4. Count how many times you had to try. **Pass: first try.**

Write down: tries, how long it took, the words on any message.

## 2. Turn it on (2 minutes)

1. On PC A, on the row for PC B: turn on **Mouse and keyboard** and **Clipboard**.
2. On PC B, on the row for PC A: turn on the same two.
3. Within 10 seconds both rows should say **Connected**, and the line under the title should say
   "Mouse sharing is on · … connected". Hover the tray icon: it should say "Mouse sharing is on".
4. Under the row, "Where PC B sits" should say **On the right** on PC A. On PC B, set PC A to **On the left**.

Write down: seconds until Connected (both PCs), what the tray said.

## 3. 50 crossings (6 minutes) — check N6.2

1. Open Notepad on both PCs.
2. On PC A push the pointer to the right edge. It should appear on PC B at about the same height, a few pixels in.
   The line should say **Mouse is on PC B** (and on PC B: "PC A is using this computer").
3. Type "the quick brown fox 123", click, scroll with the wheel, hold Shift and use the arrows, press Ctrl+C and Ctrl+V.
   All of it should happen on PC B only.
4. Push the pointer back through PC B's left edge. It should be home on PC A.
5. Repeat until you have counted **50 crossings** (every pass over an edge is one). Keep a tally.

**Pass:** all 50 crossed, no key stuck, no "Offline" on either page at any time.

Write down: crossings counted, misses (the pointer did not cross), flickers, any wrong or missing key, anything that
felt slow.

## 4. An 8 MB picture (4 minutes) — check N6.3

1. On PC A open a picture of at least 8 MB (a large PNG) and copy it (Ctrl+C).
2. Push the pointer to PC B. While the picture goes across, keep moving the pointer: it must not freeze or jump.
3. In Paint on PC B press Ctrl+V. The picture should be there, whole.
4. Copy some text on PC B, push the pointer back to PC A and paste. It should arrive on the way home.
5. Copy a picture over 32 MB on PC A and cross. It should **not** go; the page says "That copy is too big to share
   between computers (32 MB limit)." and the link stays up.

**Pass:** the picture pasted whole, the pointer never froze, the page never said Offline.

Write down: seconds from crossing to paste, whether the pointer hiccuped, what the page said for the 32 MB picture.

## 5. Pull the cable (4 minutes) — check N6.4

1. Open Notepad on PC B. Push the pointer there. **Hold the A key** on PC A (it types aaaa on PC B).
2. While holding, pull the network cable on PC A (or switch Wi-Fi off). Keep holding the key until the letters stop.
3. The letters must stop **within about a third of a second**. Film it on your phone in slow motion if you can.
4. The pointer must be back home on PC A within a second, without you doing anything.
5. Press and let go of Shift, Ctrl, Alt and the Windows key on both PCs. Nothing should be stuck, and no menu should
   pop up.
6. After about 3 seconds the row for the other PC should say **Offline** on both PCs.
7. Put the cable back. Do not click anything. **Both rows should say Connected again by themselves.**
8. Do it once more, this time pulling the cable at the moment the pointer crosses the edge.

**Pass:** letters stop in under a third of a second, the pointer comes home, no stuck keys, Offline within about 3 s,
reconnect without a click.

Write down: how long the letters kept coming, seconds to Offline, seconds to Connected again, any stuck key.

## 6. Panic, pause, off and quit (4 minutes)

1. Pointer on PC B, hold Ctrl and Alt, press **Esc**. Pointer home, nothing stuck. (Panic.)
2. Pointer on PC B, press **Scroll Lock**. Pointer home, the page says "Mouse sharing is paused". Push at the edge:
   nothing crosses. Press **Resume sharing**: it crosses again.
3. Pointer on PC B. You drive PC B with PC A's mouse, so use it on PC B's page: turn **Mouse and keyboard** off for
   PC A. The link closes and the pointer must be home on PC A at once. Turn it on again, on PC B.
4. Pointer on PC B. Quit Puppet Master on PC B (tray icon, Quit). The pointer must come home on PC A. Start it again.
5. Pointer on PC B. On PC A press **Ctrl + Alt + Del**, open Task Manager and end Puppet Master (a crash). PC A must
   take its mouse and keyboard back at once. The pointer stays in the middle of the screen; that is expected.
   Start Puppet Master again.

Write down: anything that did not work first time.

## 7. Windows that run as admin (1 minute)

1. On PC B open Notepad **as administrator**. Push the pointer to PC B and click in it and type.
2. Windows will ignore the typing. That is expected. The page says "Can't control windows that run as admin."

Write down: what happened, and if the page said the line.

## 8. One hour idle (leave it running) — check N6.5

1. Start this last. Both PCs **Connected**, pointer on PC A. Note the time.
2. Do not touch them for one hour (other work on PC A is fine). Sleep stays off.
3. After the hour: both rows still **Connected**? Did either page say Offline at any time (look at the line under the
   title; if you were away, check whether the rows show a recent reconnect)? Do one crossing both ways.

**Pass:** no Offline during the hour, a crossing works at the end.

Write down: still Connected yes/no, any Offline you saw, the time of the last crossing.

## Optional: the one-PC soak (2 minutes)

On one PC, in `packages/app/src-tauri`: `cargo run --example desk_soak -- --secs 60 --copy-mb 8`.
Two linked computers on loopback with a pretend PC. It prints a report and **PASS** or **FAIL**. It does not touch
your real pointer or keyboard. Write down the report: offline samples (want 0), longest heartbeat gap (want under
150 ms), copies arrived.

## What to write down

| Check | Result (pass / fail) | Numbers and notes |
|---|---|---|
| Build ids (A / B) | | |
| Wired or Wi-Fi (A / B) | | |
| 1. Pair first try | | tries: , time: |
| 2. Connected within 10 s | | A: s, B: s, tray said: |
| 3. 50 crossings | | counted: , misses: , stuck keys: |
| 4. 8 MB picture | | seconds: , pointer froze? , 32 MB message: |
| 5. Cable pull: letters stopped after | | ms: , Offline after: s, Connected again after: s |
| 5. Cable pull at the edge | | |
| 6. Panic / pause / switch off / quit | | |
| 7. Admin window | | |
| 8. One hour idle | | Offline during the hour? |
| Antivirus / SmartScreen warnings | | |
| Optional soak report | | |

Send the sheet back as it is. Add the Puppet Master log if anything failed.

## Known limits (not failures)

- After **restarting** Puppet Master, a pair of PCs does not find each other again by themselves: the link remembers
  where the other PC is only while it runs, and the nearby-computers search is not switched on yet. Use **Add a
  computer** again if that happens, and write down that it did.
- Files, drag and drop between computers, and Mac: not in this version.
- The pointer is held still in the middle of the screen while it is on the other PC. It is not hidden.
