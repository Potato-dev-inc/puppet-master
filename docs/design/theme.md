# Theme: charcoal and a little Potato orange

Founder, 2026-11-01: "migrate from pure black to some more shades of different colors, add in a scent of orange and
different shades of orange, as our potato bot is orange, only in small places, about 1-5%."

The direction of `mockups/ui-proposal/STYLE.md` still holds: quiet, Cursor and Codex density, Apple finish, state
shown by glyph, weight and motion. This page changes two things only: the dark surfaces are layered charcoal instead
of near-black, and there is one accent colour, Potato orange, used in a few small places. Every other colour rule in
STYLE.md (no red, green or blue; no gradients; no coloured badges) is unchanged.

## Where the tokens live

Five token roots carry the same names and values. Keep them in step.

| File | Root | Light theme selector |
|---|---|---|
| `styles/cockpit.css` | `.cockpit` | `.cockpit[data-theme='light']` |
| `styles/phone.css` (phone PWA) | `.ph` | `:root[data-theme='light'] .ph` |
| `styles/askbar.css` | `.askbar` | `@media (prefers-color-scheme: light)` |
| `styles/point-box.css` | `.point-box` | `@media (prefers-color-scheme: light)` |
| `styles/settings.css` | `.settings` | `[data-theme='light'] .settings` (calls the grounds `--window` and `--list`) |
| `styles/theme.css` | `:root` | `:root[data-theme='light']` (the older `--pm-*` triplets: terminal canvas, Tailwind blocks) |

Colour literals live only in these files. Feature stylesheets use `var(--token)` (several have a test that forbids
literals). `close-confirm.css` is the one standalone exception: the pop-out close sheet loads without tokens.

## Dark surfaces

Elevation is lightness: the higher a surface sits, the lighter it is. A very small warm tilt (red a touch above blue)
keeps the greys friendly beside orange.

| Token | Value | Used for |
|---|---|---|
| `--bg` (`--window`) | `#1b1a19` | the app background, the terminal, the main column |
| `--bg-side` | `#211f1e` | the sidebar, the bots list |
| `--bg-raised` (`--list`) | `#292725` | cards, inputs, the composer, grouped lists |
| `--bg-float` | `rgba(52, 50, 47, 0.88)` (`0.94` in the point box) | menus, palette, sheets (with backdrop blur) |
| `--fill`, `-hover`, `-active` | warm white at `0.055`, `0.09`, `0.14` | hover and selected row backgrounds |
| `--sep` | warm white at `0.10` | 1px hairlines |
| `--label` | `#f4f2ef` | text |
| `--label-2`, `-3`, `-4` | `#f4f2ef` at `0.68`, `0.58`, `0.26` | secondary text, hints, disabled |
| `--pm-bg`, `--pm-panel`, `--pm-raised`, `--pm-border` | `27 26 25`, `33 31 30`, `41 39 37`, `62 59 56` | same greys as RGB triplets |

The old values were `#0a0a0b`, `#111112`, `#18181a`. The terminal canvas reads `--pm-bg`, so it moved with them.
The light theme surfaces are unchanged (`#ffffff`, `#f5f5f7`).

## The accent

| Token | Dark | Light | Meaning |
|---|---|---|---|
| `--accent` | `#f29a3c` | `#a84f05` | fills and text |
| `--accent-hover` | `#f7ad5c` | `#8f4204` | hover of an accent fill or link |
| `--accent-press` | `#d9852b` | `#763603` | pressed |
| `--accent-soft` | `rgba(242,154,60,0.14)` | `rgba(168,79,5,0.10)` | a faint tint behind something (defined, free to use) |
| `--accent-line` | `#f29a3c` | `#d4690b` | 1-2 px markers and rings (3:1 is enough, so light can be brighter) |
| `--accent-fg` | `#1b1409` | `#ffffff` | text on an accent fill |
| `--focus` | accent at 0.40 | `rgba(212,105,11,0.35)` | the 3px focus ring |
| `--pm-accent` | `242 154 60` | `168 79 5` | the same colour as an RGB triplet (terminal cursor, older blocks) |

The dark orange is the Potato art's amber (`#F0A845`) pushed a few degrees toward orange so it reads as orange next to
charcoal. The light one is darker so that it is still readable as text on white.

### Where it is used

Budget: about 1 to 5 percent of the pixels. Never a large background, body text, a card or every button.

1. Focus ring: `--focus` on every focused button and input (cockpit, phone, ask bar, settings, point box), and the
   chat composer's border while it holds focus.
2. Sidebar: the active row gets a 2px bar at its left edge (`.ck-row.is-active::before`).
3. Segmented control: the selected pill has a 2px line along its bottom edge (`.ck-seg-pill::after`). Phone: the
   selected tab (`.ph-tab[aria-selected='true'] .ph-tab-in::after`).
4. Composer send buttons: the chat composer and the Potato Bot composer (`.ck-btn.ck-chat-send`), and the phone chat
   send button. They are the only buttons with an orange fill.
5. Needs you: the count badge in the sidebar (`.ck-count`: Inbox, Potato Bot unread) and on the phone (`.ph-count`),
   the dot in the "Needs you" bar above a terminal (`.ck-dock-bar-dot`), and the Potato Bot "Needs you" pill
   (`.ck-potato-pill--strong`).
6. Potato Bot identity: the Potato Bot icon in the sidebar (`.ck-row-glyph--potato`) and a 1.5px ring round each bot's
   face in the bots list (`.ck-potato-conv-face`).
7. Unread: the dot on an unread bot conversation (`.ck-potato-dot`).
8. DotSpinner: the working sweep and the Needs-you plus (`.pm-dots-s-running`, `.pm-dots-s-waiting_input`). Idle,
   error and done glyphs stay in the text colour.
9. Links: chat links turn orange on hover (`.ck-chat-link:hover`).
10. Terminal cursor: the canvas terminal's cursor is `--pm-accent`.

## Contrast

`styles/label-contrast.test.ts` reads the tokens from the CSS and fails below these numbers.

| Pair | Ratio (dark) | Ratio (light) | Needs |
|---|---|---|---|
| `--label-2` on bg / side / raised / float | 7.75 / 7.46 / 6.96 / 6.42 | 7.13 / 6.81 / 7.13 / 7.00 | 4.5 |
| `--label-3` on bg / side / raised / float | 5.98 / 5.81 / 5.49 / 5.12 | 5.07 / 4.90 / 5.07 / 5.00 | 4.5 |
| `--label-3` in Settings (light: window, sidebar, list, float) | | 4.90 / 4.75 / 5.07 / 4.97 | 4.5 |
| `--accent` as text on bg / side / raised / float | 7.83 / 7.40 / 6.71 / 6.00 | 5.55 / 5.09 / 5.55 / 5.36 (4.71 on the Settings sidebar) | 4.5 |
| `--accent-fg` on `--accent` / hover / press | 8.22 / 9.64 / 6.38 | 5.55 / 7.12 / 9.13 | 4.5 |
| `--accent-line` as a marker on bg / side / raised / float | 7.83 / 7.40 / 6.71 / 6.00 | 3.61 / 3.31 / 3.61 / 3.48 (3.06 on the Settings sidebar) | 3 |
