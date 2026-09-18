import { onRequestGet } from '../../functions/api/search.js';

// Cloudflare Pages handles this route in production. Use the same handler in
// Astro dev so catalog search can be exercised without deployment.
export function catalogSearchApi() {
  return {
    name: 'devenv-catalog-search-api',
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const url = new URL(req.url, 'http://localhost');
        if (url.pathname !== '/api/search') return next();
        if (req.method !== 'GET') {
          res.writeHead(405, { Allow: 'GET' });
          res.end();
          return;
        }
        const controller = new AbortController();
        const abort = () => controller.abort();
        res.on('close', abort);
        try {
          const response = await onRequestGet({ request: new Request(url, { signal: controller.signal }) });
          res.writeHead(response.status, Object.fromEntries(response.headers));
          res.end(await response.text());
        } catch {
          if (!res.destroyed) { res.writeHead(502); res.end('Search unavailable'); }
        } finally {
          res.off('close', abort);
        }
      });
    },
  };
}
