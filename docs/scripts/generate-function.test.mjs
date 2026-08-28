import assert from 'node:assert/strict';
import test from 'node:test';

import { createGenerateHandler } from '../../functions/api/generate.js';

function request(query = '') {
  return new Request(`https://devenv.sh/api/generate${query}`);
}

async function payload(response) {
  return response.json();
}

test('the generation proxy validates prompts without contacting upstream', async () => {
  let calls = 0;
  const handler = createGenerateHandler({ upstreamFetch: async () => {
    calls += 1;
    return new Response();
  } });

  const missing = await handler({ request: request() });
  assert.equal(missing.status, 400);
  assert.equal(missing.headers.get('Cache-Control'), 'no-store');
  assert.equal((await payload(missing)).error, 'Describe the environment you want to generate');

  const long = await handler({ request: request(`?q=${'x'.repeat(1001)}`) });
  assert.equal(long.status, 400);
  assert.equal((await payload(long)).error, 'The environment description is too long');
  assert.equal(calls, 0);
});

test('the generation proxy forwards a trimmed, encoded prompt and filters its response', async () => {
  let upstreamUrl = '';
  const handler = createGenerateHandler({ upstreamFetch: async (url, init) => {
    upstreamUrl = url;
    assert.equal(init.method, 'POST');
    assert.equal(init.headers.Accept, 'application/json');
    assert.ok(init.signal instanceof AbortSignal);
    return Response.json({ devenv_nix: '  languages.rust.enable = true;  ', devenv_yaml: 42, ignored: 'value' });
  } });
  const response = await handler({ request: request('?q=%20Rust%20%26%20Redis%20') });

  assert.equal(response.status, 200);
  assert.equal(response.headers.get('Cache-Control'), 'no-store');
  assert.equal(response.headers.get('X-Content-Type-Options'), 'nosniff');
  assert.equal(new URL(upstreamUrl).searchParams.get('q'), 'Rust & Redis');
  assert.deepEqual(await payload(response), {
    devenv_nix: '  languages.rust.enable = true;  ',
    devenv_yaml: '',
  });
});

test('the generation proxy preserves a generated devenv.yaml companion file', async () => {
  const yaml = `inputs:
  rust-overlay:
    url: github:oxalica/rust-overlay
`;
  const handler = createGenerateHandler({ upstreamFetch: async () => Response.json({
    devenv_nix: 'languages.rust.enable = true;',
    devenv_yaml: yaml,
  }) });
  const response = await handler({ request: request('?q=old%20rust') });

  assert.equal(response.status, 200);
  assert.equal((await payload(response)).devenv_yaml, yaml);
});

test('the generation proxy handles upstream, payload, and size failures', async (context) => {
  await context.test('upstream failure', async () => {
    const handler = createGenerateHandler({ upstreamFetch: async () => new Response('', { status: 503 }) });
    const response = await handler({ request: request('?q=rust') });
    assert.equal(response.status, 502);
    assert.equal(response.headers.get('Retry-After'), '30');
  });

  await context.test('invalid JSON', async () => {
    const handler = createGenerateHandler({ upstreamFetch: async () => new Response('{') });
    assert.equal((await handler({ request: request('?q=rust') })).status, 502);
  });

  await context.test('empty Nix', async () => {
    const handler = createGenerateHandler({ upstreamFetch: async () => Response.json({ devenv_nix: '  ' }) });
    const response = await handler({ request: request('?q=rust') });
    assert.equal(response.status, 502);
    assert.equal((await payload(response)).error, 'AI returned an empty devenv.nix');
  });

  await context.test('oversized payload', async () => {
    const handler = createGenerateHandler({
      maxResponseBytes: 32,
      upstreamFetch: async () => Response.json({ devenv_nix: 'x'.repeat(64) }),
    });
    assert.equal((await handler({ request: request('?q=rust') })).status, 502);
  });
});

test('the generation proxy aborts stalled upstream requests', async () => {
  let aborted = false;
  const handler = createGenerateHandler({
    timeoutMs: 5,
    upstreamFetch: async (_url, init) => new Promise((_resolve, reject) => {
      init.signal.addEventListener('abort', () => {
        aborted = true;
        reject(init.signal.reason);
      }, { once: true });
    }),
  });
  const response = await handler({ request: request('?q=rust') });
  assert.equal(response.status, 504);
  assert.equal((await payload(response)).error, 'AI generation timed out');
  assert.equal(aborted, true);
});
