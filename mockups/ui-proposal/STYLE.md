# UI proposal — style spec (v2)

Direction: **monochrome, minimalist, tool-like** (think Cursor / Codex / Linear density), **finished like Apple**
(macOS System Settings, iOS grouped lists, SF typography, hairlines, restraint). This replaces the coloured
Liquid Glass look in DESIGN.md for these mockups.

## Hard bans
- No hue anywhere. Grayscale only (state is shown by glyph, weight and motion, never colour).
- No gradients, glows, neon, coloured badges, emoji, uppercase-tracked labels, decorative borders.
- No shadows except floating surfaces (popover, sheet, palette, toast).
- No card-inside-card. Prefer rows separated by hairlines over boxes.
- No marketing copy. Sentence case, short labels, Apple-style microcopy.

## Tokens (dark default; light under `prefers-color-scheme: light`)
```css
:root {
  --bg: #0a0a0b;          /* window / terminal */
  --bg-side: #111112;     /* sidebar */
  --bg-raised: #18181a;   /* grouped list rows, inputs */
  --bg-float: rgba(28,28,30,.82); /* popovers/sheets, with backdrop-filter: blur(24px) saturate(1.4) */
  --fill: rgba(255,255,255,.05);
  --fill-hover: rgba(255,255,255,.08);
  --fill-active: rgba(255,255,255,.12);
  --sep: rgba(255,255,255,.08);   /* 1px hairline */
  --label: #f5f5f7;
  --label-2: rgba(245,245,247,.64);
  --label-3: rgba(245,245,247,.40);
  --label-4: rgba(245,245,247,.22);
  --primary-bg: #f5f5f7;  --primary-fg: #0a0a0b;  /* the only "accent" is white */
  --focus: 0 0 0 3px rgba(245,245,247,.28);
}
@media (prefers-color-scheme: light) {
  :root {
    --bg: #ffffff; --bg-side: #f5f5f7; --bg-raised: #ffffff; --bg-float: rgba(250,250,252,.82);
    --fill: rgba(0,0,0,.04); --fill-hover: rgba(0,0,0,.06); --fill-active: rgba(0,0,0,.09);
    --sep: rgba(0,0,0,.09);
    --label: #1d1d1f; --label-2: rgba(29,29,31,.64); --label-3: rgba(29,29,31,.42); --label-4: rgba(29,29,31,.24);
    --primary-bg: #1d1d1f; --primary-fg: #ffffff; --focus: 0 0 0 3px rgba(0,0,0,.18);
  }
}
```

## Type
- UI: `-apple-system, BlinkMacSystemFont, "SF Pro Text", "Inter", "Segoe UI Variable Text", system-ui, sans-serif`
  (load **Inter** from Google Fonts as the cross-platform fallback; `font-feature-settings: "cv11","ss01"`).
- Mono: `"SF Mono", "JetBrains Mono", "Cascadia Code", ui-monospace, monospace` (load JetBrains Mono).
- `-webkit-font-smoothing: antialiased`. Tabular numbers for sizes/timers.
- **Desktop scale**: 11px caption (label-3) · 13px body (default) · 13px/600 row title · 15px/600 section · 22px/600 page title (letter-spacing -0.01em). Terminal 12.5px / line-height 1.5.
- **Phone scale** (iOS): 13 caption · 15 secondary · 17 body · 20/600 nav title · 34/700 large title (-0.02em).

## Shape, space, motion
- Radius: 6 controls · 8 rows/inputs · 10 grouped lists · 12 popovers & windows · 14 sheets (phone 12 + 38 top for sheets) · 999 pills.
- 4px grid. Desktop rows 28–32px high, sidebar 240px. Phone rows 44px minimum, 16px insets.
- Icons: inline SVG, 16px, 1.5px stroke, round caps (SF Symbols feel). One icon set, no fills except state glyphs.
- Motion: 160ms `cubic-bezier(.2,.8,.2,1)` for hover/press; sheets/palette 280ms `cubic-bezier(.32,.72,0,1)`; press = scale(.97). Honour `prefers-reduced-motion` (no transforms, no pulse, no spinners → static glyphs).

## State vocabulary (one set, everywhere: sidebar, tab, header, tray)
| State | Glyph (12px) | Text |
|---|---|---|
| Needs you | solid filled dot in `--label` + slow 2s ring pulse; row gets `--fill` bg and title weight 600 | "Needs you" |
| Running / tool / writing | 3/4 arc spinner, 1.5px, `--label-2`, 0.9s linear | "Running" / "Running tool" / "Writing" |
| Thinking | three 2px dots breathing, `--label-2` | "Thinking" |
| Idle | hollow 1.5px ring, `--label-3` | "Idle" |
| Error | small "!" inside a hollow rounded square, `--label` | "Error" |
| Done | checkmark, `--label-3` | "Done" |
Needs-you is the only thing allowed to be visually loud, and only through contrast.

## Components (reuse names)
- **Sidebar list** (Cursor-like): section captions ("Needs you", "Working", "Idle") in 11px label-3; rows = state glyph · agent name (13/500) · project (label-3) · right-aligned elapsed (tabular, label-3). Selected row: `--fill-active`, radius 6.
- **Composer**: pill/rounded-12 field at the bottom of a pane, `--bg-raised`, 1px `--sep` border, placeholder "Reply to Claude Code…", circular 28px send button (primary-bg, ↑ arrow). Pending agent question appears *above* it as a quiet row: question text + segmented answer buttons (first = primary).
- **Buttons**: primary (white fill), secondary (`--fill`), plain (text only). 28px desktop / 44px phone.
- **Segmented control**, **switch** (Apple style, monochrome: on = `--label` track, `--bg` knob), **grouped inset list** (System Settings), **popover menu** with ⌘ shortcuts right-aligned in label-3, **command palette** (⌘K, 560px, float bg).
- **Terminal**: same `--bg` as window, `--label-2` text, prompts/agent bullets in `--label`, diff `+`/`-` lines shown by `--label` vs `--label-3` + a 2px left rule, never red/green.
- Every mock surface carries a quiet "Example data" caption (11px label-4).
