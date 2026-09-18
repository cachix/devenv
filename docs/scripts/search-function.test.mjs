import assert from 'node:assert/strict';
import test from 'node:test';
import { createSearchHandler, readRpc } from '../../functions/api/search.js';

const request = (tool = 'search_packages', q = 'postgres') => new Request(`https://devenv.sh/api/search?${new URLSearchParams({ tool, q })}`);
function upstream(data, { toolError = false, rpcError = false } = {}) {
  const calls = [];
  return { calls, fetch: async (url, init) => {
    assert.equal(url, 'https://mcp.devenv.sh/mcp');
    if (init.method === 'DELETE') { calls.push('DELETE'); return new Response(null, { status: 204 }); }
    const body = JSON.parse(init.body);
    calls.push(body.method);
    if (body.method === 'initialize') return Response.json({ id: body.id, result: {} }, { headers: { 'mcp-session-id': 'session' } });
    assert.equal(init.headers['Mcp-Session-Id'], 'session');
    if (body.method === 'notifications/initialized') return new Response(null, { status: 202 });
    assert.deepEqual(body.params.arguments, { query: 'postgres' });
    const result = { content: [{ type: 'text', text: JSON.stringify(data) }], isError: toolError };
    return new Response(`data: \n\ndata: ${JSON.stringify(rpcError ? { id: body.id, error: { code: -1 } } : { id: body.id, result })}\n\n`, { headers: { 'Content-Type': 'text/event-stream' } });
  } };
}
test('validates query and allowlists tools without contacting MCP', async () => {
  const handler = createSearchHandler({ upstreamFetch: () => assert.fail('unexpected request') });
  for (const req of [request('delete_all'), request('search_packages', ''), request('search_options', 'x'.repeat(201))]) {
    const response = await handler({ request: req });
    assert.equal(response.status, 400);
    assert.equal(response.headers.get('Cache-Control'), 'no-store');
  }
});
test('initializes MCP, parses streamed search results, and closes the session', async () => {
  const data = [{ name: 'pkgs.postgresql', version: '18' }];
  const mock = upstream(data);
  const response = await createSearchHandler({ upstreamFetch: mock.fetch })({ request: request() });
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { data });
  assert.match(response.headers.get('Cache-Control'), /s-maxage=300/);
  assert.deepEqual(mock.calls, ['initialize', 'notifications/initialized', 'tools/call', 'DELETE']);
});
test('preserves resolved option groups, alternatives, and no-match results', async () => {
  for (const data of [{ match: 'services.postgres', options: [], alternatives: [], probability: 0.9 }, { match: null, alternatives: [] }]) {
    const mock = upstream(data);
    const response = await createSearchHandler({ upstreamFetch: mock.fetch })({ request: request('resolve_option') });
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { data });
  }
});
test('upstream errors and invalid shapes fail without being cached', async () => {
  for (const mock of [upstream([], { toolError: true }), upstream([], { rpcError: true }), upstream({ unexpected: true })]) {
    const response = await createSearchHandler({ upstreamFetch: mock.fetch })({ request: request() });
    assert.equal(response.status, 502);
    assert.equal(response.headers.get('Cache-Control'), 'no-store');
    assert.equal(mock.calls.at(-1), 'DELETE');
  }
});
test('reads fragmented CRLF SSE events and cancels an open stream after the reply', async () => {
  let cancelled = false;
  const encoder = new TextEncoder();
  const chunks = [': heartbeat\r\n\r\ndata: {"id":', '2,"result":', '{"ok":true}}\r\n\r\n'];
  const body = new ReadableStream({ start(controller) { chunks.forEach(chunk => controller.enqueue(encoder.encode(chunk))); }, cancel() { cancelled = true; } });
  assert.deepEqual(await readRpc(new Response(body, { headers: { 'Content-Type': 'text/event-stream' } }), 2), { id: 2, result: { ok: true } });
  assert.equal(cancelled, true);
});
test('limits oversized MCP responses', async () => {
  await assert.rejects(readRpc(new Response('x'.repeat(100)), 2, 10), /too large/);
});
test('aborts timed out requests', async () => {
  const handler = createSearchHandler({ timeoutMs: 10, upstreamFetch: async (_url, { signal }) => new Promise((_, reject) => {
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
  }) });
  const keepAlive = setTimeout(() => {}, 1000);
  try { assert.equal((await handler({ request: request() })).status, 504); }
  finally { clearTimeout(keepAlive); }
});

test('development middleware leaves unrelated routes alone and rejects writes', async () => {
  const { catalogSearchApi } = await import('./search-dev.mjs');
  let middleware;
  const server = { middlewares: { use(handler) { middleware = handler; return this; } } };
  assert.equal(catalogSearchApi().configureServer(server), undefined);
  let next = false;
  await middleware({ url: '/packages/' }, {}, () => { next = true; });
  assert.equal(next, true);
  let status;
  let ended = false;
  await middleware({ url: '/api/search?q=postgres', method: 'POST' }, {
    writeHead(code, headers) { status = code; assert.equal(headers.Allow, 'GET'); },
    end() { ended = true; },
  }, () => assert.fail('unexpected next'));
  assert.equal(status, 405);
  assert.equal(ended, true);
});
