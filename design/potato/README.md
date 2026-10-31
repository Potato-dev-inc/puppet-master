# Potato animation pack

Potato is the founder's hand-drawn mascot: the face of the Puppet Master bot ("Potato Bot"),
its chat avatar, its states in the app and on the phone, and an optional desktop pet.
Everything here is generated from one traced rig, so it can be rebuilt at any time.

Potato artwork © the Puppet Master founder; all frames derived from reference.jpg

## What is in here

| Path | What |
| --- | --- |
| `reference.jpg` | the founder's drawing (the only source of truth) |
| `base/` | step 1: `potato-base.svg` (layered), `parts/*.svg` (body, hair, eyes, mouth, arm-left, arm-right, staff, legs), `rig.json` (points and pivots), `compare-reference.png` (reference vs ours vs overlay) |
| `gif/<state>.gif` | preview GIFs, 256 px on the black circle, 12 fps |
| `animated/<state>.svg` | self-animating SVG (CSS only, no JS), transparent, usable as `<img>` |
| `animated/<state>.webp` | animated WebP, 256 px, transparent |
| `animated-circle/<state>.svg` | the same animated SVGs on the black circle |
| `exports/svg/<state>/<NN>.svg` | every frame, transparent |
| `exports/png/<size>/<state>/<NN>.png` | every frame, transparent (sizes 128 and 512 committed) |
| `exports/png-circle/<size>/<state>/<NN>.png` | every frame on the black circle |
| `exports/sheets/<state>.png` | sprite sheets, 128 px cells, 8 columns |
| `exports/manifest.json` | per state: fps, loop or once, frame count, file lists, sheet layout |
| `exports/pet/` | desktop-pet pack: `manifest.json` in the pet-loader format plus `<state>-<NN>.png` (256x256, transparent) |
| `icons/` | app icon from the idle pose on the black circle: 16, 32, 48, 64, 128, 256, 512 PNG, `icon.ico`, `icon.svg` |
| `preview.html` | plays every state live (needs the files next to it); light / dark / checker, 64-512 px, SVG / WebP / GIF |
| `preview-standalone.html` | the same page as ONE file, all animated SVGs inlined, no external files, no CDN |
| `contact-sheet.png` | a labelled still of every state next to the reference |
| `persona/core.md` | the Potato persona text (cheerful little potato king, short chat-style replies) |
| `tools/` | the generator (Node 22) |

## The states (12 fps)

| State | Frames | Plays | What it shows |
| --- | --- | --- | --- |
| idle | 24 | loop | gentle bob and breathing |
| blink | 6 | once | one quick blink |
| talk | 12 | loop | mouth shapes closed / small / open / wide |
| listen | 18 | loop | leans in, eyes look to the side |
| think | 24 | loop | staff ring glows and orbits, "..." dots |
| working | 24 | loop | taps the staff like typing, sparks |
| happy | 12 | loop | bounce |
| celebrate | 24 | once | jump, sparkles from the staff |
| wave-hello | 24 | loop | waves the staff |
| needs-you | 24 | loop | waves the staff, orange "!" above |
| surprised | 12 | once | hair spikes jump up, O mouth |
| sad | 24 | loop | droops, hair flops, down mouth |
| error | 24 | loop | droop plus a nervous shake, flat mouth |
| sleepy | 24 | loop | half eyes, a big yawn |
| sleep | 24 | loop | closed-eye lines, floating z |
| walk-left | 12 | loop | leg cycle (desktop pet) |
| walk-right | 12 | loop | leg cycle (desktop pet) |
| dragged | 12 | loop | picked up, legs dangle (desktop pet) |
| land | 9 | once | squash and stretch landing (desktop pet) |
| loading | 12 | loop | small icon: the staff with its closed ring and a spinner of ticks |

Loops are built so the last frame leads into the first. "Once" states end on a held pose
(the GIF and WebP hold it for about a second before they restart).

## How it is drawn

- One orange (`#F0A845`, sampled from the reference) on a black circle (`#050505`) or on transparent.
- Parts are traced centre-lines in a 512 x 512 canvas where the black circle fills the canvas.
  Each part has a pivot, and poses move the points themselves (never an SVG transform), so squash
  and stretch never change the line weight.
- Line weight is 14 px of 512 (about 2.7%). The reference measures about 2.5% of the circle, so this
  matches it, a hair bolder so it still reads at 64 px. The tiny icon sizes (16 to 64) get a thicker line.
- Line boil: every line has 3 slightly different hand-drawn wobbles, cycled per frame and out of
  step between parts, so even still poses stay alive.
- Outline only, except the eyes and mouth, which are filled.

## Rebuild (one command)

```
cd design/potato/tools && npm install && npm run build
```

`npm run build` rewrites everything above (about 10 seconds). It needs Node 22 and the dev
tools in `tools/package.json` (resvg, gifenc, pngjs, jpeg-js). The animated WebPs use ffmpeg
(`C:/ffmpeg/bin/ffmpeg.exe`, or set `FFMPEG`).

- `npm run build:all` also writes the 64 and 256 px PNG frames. They are not committed, to keep the
  repo small (the committed pack is about 38 MB), so the manifest only lists 128 and 512.
- `node build.mjs --quick` redraws only `base/` and `contact-sheet.png`.
- `node build.mjs --phase=animated` writes only the GIF / SVG / WebP and the two preview pages.
- `node compare.mjs` redraws `base/compare-reference.png`.
- Edit poses in `tools/lib/states.mjs`, the traced shape in `tools/lib/rig.mjs`.
- `tools/trace/` holds the scripts used to measure the reference.

## Using it

- Chat avatar: `icons/icon-128.png` or `exports/png-circle/128/idle/01.png`.
- App and phone states: `animated/<state>.svg` or `.webp`.
- Desktop pet: point the pet loader at `exports/pet/manifest.json`.
