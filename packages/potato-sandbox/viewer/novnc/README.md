# noVNC (vendored)

This folder holds an unmodified copy of the noVNC client library.

- Project: noVNC, https://github.com/novnc/noVNC
- Package: `@novnc/novnc` on npm, **version 1.7.0** (tarball sha1 `7f832cf07c66475a81a25708b8e5299a5c4efec5`, from `npm pack @novnc/novnc@1.7.0`)
- Licence: **MPL-2.0** for the noVNC core library (`core/`). See `LICENSE.txt` here, which also lists the licences of the
  parts it covers. `vendor/pako/` is a separate library under its own licence (`vendor/pako/LICENSE`, MIT).
  The list of authors is in `AUTHORS`.

## What is here

- `core/` - the noVNC client (`rfb.js` and everything it imports). `../embed.html` loads `core/rfb.js`,
  `core/input/keyboard.js`, `core/input/keysym.js` and `core/input/keysymdef.js`.
- `vendor/pako/` - the zlib library that `core/inflator.js` and `core/deflator.js` import.
- `LICENSE.txt`, `AUTHORS` - copied as shipped.

Not copied: the package's `docs/`, its `README.md`, and the stock `vnc.html` / `app/` pages (we use our own `../embed.html`).

## Rules

- Do not edit any file here. The MPL-2.0 requires that changes to these files stay under the MPL and be published.
  Our own code (`../embed.html`, `../clipboard-bridge.js`, `../mobile-keyboard.js`) lives outside this folder and only imports from it.
- To update: `npm pack @novnc/novnc@<version>` in an empty folder, replace `core/`, `vendor/`, `LICENSE.txt` and `AUTHORS`
  with the new copies, and change the version and checksum above. Do not mix files from two versions.
- The headers inside each file (for example the noVNC copyright line in `core/rfb.js`) must stay as shipped.
