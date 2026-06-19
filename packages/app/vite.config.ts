import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { VitePWA } from 'vite-plugin-pwa';
import { resolve } from 'node:path';
import { bridgeProxyConfig } from './vite-bridge-proxy';
import { devInfoPlugin } from './vite-dev-info';

export default defineConfig({
  plugins: [
    react(),
    devInfoPlugin(),
    VitePWA({
      registerType: 'autoUpdate',
      includeAssets: ['app-icon.svg', 'icon-192.png', 'icon-512.png'],
      workbox: {
        navigateFallbackDenylist: [/^\/bridge/, /^\/__puppet_master_dev__/],
        runtimeCaching: [
          {
            urlPattern: /^\/bridge\//,
            handler: 'NetworkOnly',
          },
        ],
      },
      manifest: {
        name: 'Puppet Master',
        short_name: 'PM',
        description: 'Multi-agent terminal orchestrator',
        display: 'standalone',
        background_color: '#0f0f0f',
        theme_color: '#0f0f0f',
        icons: [
          { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
          { src: '/icon-512.png', sizes: '512x512', type: 'image/png', purpose: 'any maskable' },
        ],
      },
      devOptions: {
        enabled: true,
      },
    }),
  ],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: true,
    allowedHosts: true,
    proxy: bridgeProxyConfig,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  preview: {
    host: true,
    allowedHosts: true,
    proxy: bridgeProxyConfig,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: 'es2022',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
