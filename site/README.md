# Landing page (draft)

Static HTML and CSS, no framework, no build dependencies. The only external request is Google Fonts.

## Layout
- `src/index.template.html`: the page. `{{NAME}}` is the product name.
- `site.config.json`: the product name (`name`), repo link, footer licence line, year.
- `build.mjs`: renders the template into `index.html` (`node site/build.mjs`). `index.html` is committed so the page opens without a build.
- `css/site.css`, `js/site.js`: styles (tokens from `mockups/ui-proposal/STYLE.md`, dark and light) and the theme toggle plus the waitlist stub.
- `assets/*.webp`: product shots. `tools/prepare-assets.sh` records the source and crop of each one.
- `tools/screenshot.mjs`: headless Edge evidence at 1440 and 390 px, dark and light, and a check that no host other than Google Fonts is requested.

## Renaming (NAME-BRAND.md recommends Ninedot, not decided)
1. Edit `name` in `site.config.json`.
2. Run `node site/build.mjs`.

Not covered by that one edit, on purpose:
- The screenshots show the app's own UI, which says "Puppet Master". Re-shoot them after a rename.
- The MCP snippet in the "Agents that start agents" tile is a literal config (`puppet-master`, `@puppet-master/mcp`). It changes when the package does.
- The 3x3 dot mark is the app's state glyph, not a brand decision.

## Waitlist
The form's `action` is `#`, so nothing is sent and the page says so. To connect it, set the form's `action` and `method` in the template.
