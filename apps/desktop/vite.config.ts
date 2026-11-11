import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitest/config';

// Tauri expects a fixed port and fails if it is taken.
/**
 * Serves /preview.html while developing: the app's own page started through
 * src/preview.ts, which puts a pretend camera behind it. Not in the build.
 */
const preview = {
  name: 'rimix-preview',
  apply: 'serve' as const,
  configureServer(server: import('vite').ViteDevServer) {
    server.middlewares.use(async (req, res, next) => {
      if (!req.url?.startsWith('/preview.html')) return next();
      const page = readFileSync('index.html', 'utf8').replace('/src/main.ts', '/src/preview.ts');
      res.setHeader('Content-Type', 'text/html');
      res.end(await server.transformIndexHtml(req.url, page));
    });
  },
};

export default defineConfig({
  clearScreen: false,
  plugins: [preview],
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  test: {
    environment: 'jsdom',
  },
});
