import assert from 'node:assert/strict';
import test from 'node:test';

import { onRequest } from '../../functions/_middleware.js';

test('the Markdown middleware resolves the public Site Kit export', () => {
  assert.ok(Array.isArray(onRequest));
  assert.equal(onRequest.length, 1);
  assert.equal(typeof onRequest[0], 'function');
});

test('the Markdown middleware serves index.md when text/markdown is preferred', async () => {
  const [middleware] = onRequest;
  const assets = {
    fetch: async (request) =>
      new URL(request.url).pathname === '/basics/index.md'
        ? new Response('# Basics', { status: 200 })
        : new Response('missing', { status: 404 }),
  };
  const response = await middleware({
    request: new Request('https://devenv.sh/basics/', { headers: { Accept: 'text/markdown' } }),
    env: { ASSETS: assets },
    next: async () => new Response('<html>', { status: 200 }),
  });
  assert.equal(response.status, 200);
  assert.equal(response.headers.get('Content-Type'), 'text/markdown; charset=utf-8');
  assert.equal(response.headers.get('Content-Location'), '/basics/index.md');
  assert.equal(await response.text(), '# Basics');
});

test('the Markdown middleware falls through to HTML for browsers', async () => {
  const [middleware] = onRequest;
  const response = await middleware({
    request: new Request('https://devenv.sh/basics/', { headers: { Accept: 'text/html' } }),
    env: {},
    next: async () => new Response('<html>', { status: 200 }),
  });
  assert.equal(await response.text(), '<html>');
});
