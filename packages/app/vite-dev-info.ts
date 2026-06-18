import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Plugin } from 'vite';

const DEV_INFO_PATH = resolve(__dirname, '../../puppet-master.dev.json');
const DEV_INFO_ROUTE = '/__puppet_master_dev__.json';

/** Expose ngrok / dev URLs written by scripts/vite-dev.mjs for the PWA setup screen. */
export function devInfoPlugin(): Plugin {
  return {
    name: 'puppet-master-dev-info',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (req.url !== DEV_INFO_ROUTE) return next();
        res.setHeader('Content-Type', 'application/json; charset=utf-8');
        res.setHeader('Cache-Control', 'no-store');
        if (!existsSync(DEV_INFO_PATH)) {
          res.statusCode = 404;
          res.end(JSON.stringify({ error: 'dev info not available' }));
          return;
        }
        res.statusCode = 200;
        res.end(readFileSync(DEV_INFO_PATH, 'utf-8'));
      });
    },
    configurePreviewServer(server) {
      server.middlewares.use((req, res, next) => {
        if (req.url !== DEV_INFO_ROUTE) return next();
        res.setHeader('Content-Type', 'application/json; charset=utf-8');
        res.setHeader('Cache-Control', 'no-store');
        if (!existsSync(DEV_INFO_PATH)) {
          res.statusCode = 404;
          res.end(JSON.stringify({ error: 'dev info not available' }));
          return;
        }
        res.statusCode = 200;
        res.end(readFileSync(DEV_INFO_PATH, 'utf-8'));
      });
    },
  };
}
