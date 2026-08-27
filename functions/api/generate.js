const defaultEndpoint = 'https://devenv.new/api/generate';
const defaultTimeoutMs = 30000;
const defaultMaxResponseBytes = 256 * 1024;
const responseHeaders = {
  'Cache-Control': 'no-store',
  'X-Content-Type-Options': 'nosniff',
};

function json(payload, status = 200, headers = {}) {
  return Response.json(payload, {
    status,
    headers: { ...responseHeaders, ...headers },
  });
}

async function readJson(response, maxBytes) {
  const declaredSize = Number(response.headers.get('Content-Length'));
  if (Number.isFinite(declaredSize) && declaredSize > maxBytes) throw new Error('Response is too large');
  if (!response.body) throw new Error('Response has no body');

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let size = 0;
  let text = '';
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > maxBytes) {
      await reader.cancel();
      throw new Error('Response is too large');
    }
    text += decoder.decode(value, { stream: true });
  }
  text += decoder.decode();
  return JSON.parse(text);
}

export function createGenerateHandler({
  upstreamFetch = globalThis.fetch,
  endpoint = defaultEndpoint,
  timeoutMs = defaultTimeoutMs,
  maxResponseBytes = defaultMaxResponseBytes,
} = {}) {
  return async function onRequestPost({ request }) {
    const prompt = new URL(request.url).searchParams.get('q')?.trim();
    if (!prompt) return json({ error: 'Describe the environment you want to generate' }, 400);
    if (prompt.length > 1000) return json({ error: 'The environment description is too long' }, 400);

    const controller = new AbortController();
    const abortFromRequest = () => controller.abort(request.signal.reason);
    if (request.signal.aborted) abortFromRequest();
    else request.signal.addEventListener('abort', abortFromRequest, { once: true });
    const timeout = setTimeout(() => controller.abort(new DOMException('AI generation timed out', 'TimeoutError')), timeoutMs);

    try {
      const query = new URLSearchParams({ q: prompt });
      const response = await upstreamFetch(`${endpoint}?${query}`, {
        method: 'POST',
        headers: { Accept: 'application/json' },
        signal: controller.signal,
      });
      if (!response.ok) {
        return json({ error: 'AI generation is temporarily unavailable' }, 502, { 'Retry-After': '30' });
      }

      const payload = await readJson(response, maxResponseBytes);
      if (typeof payload?.devenv_nix !== 'string' || !payload.devenv_nix.trim()) {
        return json({ error: 'AI returned an empty devenv.nix' }, 502);
      }

      return json({
        devenv_nix: payload.devenv_nix,
        devenv_yaml: typeof payload.devenv_yaml === 'string' ? payload.devenv_yaml : '',
      });
    } catch {
      if (controller.signal.reason?.name === 'TimeoutError') {
        return json({ error: 'AI generation timed out' }, 504, { 'Retry-After': '30' });
      }
      return json({ error: 'AI generation is temporarily unavailable' }, 502, { 'Retry-After': '30' });
    } finally {
      clearTimeout(timeout);
      request.signal.removeEventListener('abort', abortFromRequest);
    }
  };
}

export const onRequestPost = createGenerateHandler();
