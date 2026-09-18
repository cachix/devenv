const endpoint = 'https://mcp.devenv.sh/mcp';
const tools = new Set(['search_packages', 'search_options', 'resolve_package', 'resolve_option']);

// Read one JSON-RPC response, including servers that keep the SSE stream open.
export async function readRpc(response, id, maxBytes = 8 * 1024 * 1024) {
  if (!response.ok || !response.body) throw new Error('MCP request failed');
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const sse = response.headers.get('content-type')?.includes('text/event-stream');
  let buffer = '';
  let bytes = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      bytes += value?.byteLength ?? 0;
      if (bytes > maxBytes) throw new Error('MCP response too large');
      buffer += decoder.decode(value, { stream: !done });
      if (sse) {
        const events = buffer.replace(/\r\n/g, '\n').split('\n\n');
        buffer = events.pop() ?? '';
        if (done && buffer) events.push(buffer);
        for (const event of events) {
          const data = event.split('\n').filter(line => line.startsWith('data:')).map(line => line.slice(5).trimStart()).join('\n');
          if (!data.trim()) continue;
          const payload = JSON.parse(data);
          if (payload.id === id) return payload;
        }
      } else if (done) {
        const payload = JSON.parse(buffer);
        if (payload.id === id) return payload;
      }
      if (done) throw new Error('Missing MCP response');
    }
  } finally {
    await reader.cancel();
  }
}

export function createSearchHandler({ upstreamFetch = globalThis.fetch, timeoutMs = 15000 } = {}) {
  return async function onRequestGet({ request }) {
    const params = new URL(request.url).searchParams;
    const query = params.get('q')?.trim();
    const tool = params.get('tool');
    const json = (body, status = 200) => Response.json(body, { status, headers: {
      'Cache-Control': status === 200 ? 'public, max-age=60, s-maxage=300' : 'no-store',
      'X-Content-Type-Options': 'nosniff',
    } });
    if (!query || query.length > 200 || !tools.has(tool)) return json({ error: 'Choose a search type and enter 1–200 characters.' }, 400);
    const signal = AbortSignal.any([request.signal, AbortSignal.timeout(timeoutMs)]);
    let session;
    const headers = () => ({
      'Content-Type': 'application/json', Accept: 'application/json, text/event-stream',
      'MCP-Protocol-Version': '2025-03-26', ...(session ? { 'Mcp-Session-Id': session } : {}),
    });
    const rpc = async (method, params, id) => {
      const response = await upstreamFetch(endpoint, {
        method: 'POST', headers: headers(), signal,
        body: JSON.stringify({ jsonrpc: '2.0', ...(id === undefined ? {} : { id }), method, params }),
      });
      session ||= response.headers.get('mcp-session-id');
      if (id === undefined) {
        await response.body?.cancel();
        if (!response.ok) throw new Error('MCP notification failed');
        return;
      }
      const payload = await readRpc(response, id);
      if (payload.error) throw new Error('MCP error');
      return payload.result;
    };
    try {
      await rpc('initialize', {
        protocolVersion: '2025-03-26', capabilities: {},
        clientInfo: { name: 'devenv-docs-search', version: '1.0' },
      }, 1);
      await rpc('notifications/initialized', {});
      const result = await rpc('tools/call', { name: tool, arguments: { query } }, 2);
      if (result?.isError) throw new Error('Search failed');
      const content = result?.content?.find(item => item.type === 'text');
      const data = JSON.parse(content?.text ?? 'null');
      if (tool.startsWith('search_') ? !Array.isArray(data) : !data || !('match' in data)) throw new Error('Invalid search response');
      return json({ data });
    } catch {
      return json({ error: signal.aborted ? 'Search timed out. Try again.' : 'Search is temporarily unavailable. Try again.' }, signal.aborted ? 504 : 502);
    } finally {
      if (session) {
        try {
          const response = await upstreamFetch(endpoint, { method: 'DELETE', headers: headers(), signal: AbortSignal.timeout(2000) });
          await response.body?.cancel();
        } catch { /* Session cleanup must not discard search results. */ }
      }
    }
  };
}

export const onRequestGet = createSearchHandler();
