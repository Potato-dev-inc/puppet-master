import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    // Node by default: a test that needs `document` or `window` says so on its first line with
    // `// @vitest-environment jsdom`. Building a jsdom window for every file cost about 40 % of a full run
    // (see docs/DEV-SPEED.md); most of the 470 files never touch the DOM.
    environment: 'node',
    include: ['src/**/*.test.{ts,tsx}', 'vite-bridge-proxy.test.ts'],
  },
});
