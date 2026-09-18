// Run against built docs with /api/search enabled and a Chromium CDP instance:
// SEARCH_BASE_URL=http://localhost:4398 SEARCH_CDP_URL=http://localhost:9237 node scripts/search-browser.mjs
import assert from 'node:assert/strict';

const base = process.env.SEARCH_BASE_URL;
const cdpUrl = process.env.SEARCH_CDP_URL;
if (!base || !cdpUrl) throw new Error('Set SEARCH_BASE_URL and SEARCH_CDP_URL');
const target = await (await fetch(`${cdpUrl}/json/new?about:blank`, { method: 'PUT' })).json();
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise(resolve => socket.addEventListener('open', resolve, { once: true }));
let nextId = 0;
const pending = new Map();
socket.addEventListener('message', ({ data }) => {
  const message = JSON.parse(data);
  if (!message.id) return;
  const call = pending.get(message.id);
  pending.delete(message.id);
  if (message.error) call.reject(message.error);
  else call.resolve(message.result);
});
function cdp(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const response = await cdp('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (response.exceptionDetails) throw new Error(JSON.stringify(response.exceptionDetails));
  return response.result.value;
}
async function until(expression) {
  const deadline = Date.now() + 25000;
  while (Date.now() < deadline) {
    if (await evaluate(expression)) return;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out: ${expression}`);
}
try {
  await cdp('Page.navigate', { url: `${base}/packages/` });
  await until(`document.querySelector('site-search input') && document.querySelector('[data-docs-heading]')`);
  await evaluate(`
    window.catalogCalls = [];
    const originalFetch = window.fetch;
    window.fetch = (...args) => {
      if (String(args[0]).startsWith('/api/search?')) window.catalogCalls.push(new URL(args[0], location.href).searchParams.get('tool'));
      return originalFetch(...args);
    };
    document.querySelector('[data-open-modal]').click();
    window.setSearchQuery = value => {
      const input = document.querySelector('site-search input');
      input.value = value;
      input.dispatchEvent(new Event('input', { bubbles: true }));
    };
    setSearchQuery('postgres');
  `);
  await until(`document.querySelector('.pagefind-ui__drawer .pagefind-ui__result-link') && document.querySelector('[data-group="options"] [data-results] li') && document.querySelector('[data-group="packages"] [data-results] li')`);
  assert.deepEqual((await evaluate('window.catalogCalls')).sort(), ['resolve_option', 'resolve_package', 'search_options', 'search_packages']);
  assert.equal(await evaluate(`document.querySelector('site-search [role="tab"]')`), null);
  assert.notEqual(await evaluate(`getComputedStyle(document.querySelector('.pagefind-ui__drawer')).display`), 'none');
  assert.equal(await evaluate(`document.querySelector('#devenv-search-results').hidden`), false);
  await until(`document.querySelector('[data-group="options"] [data-suggestion] a') && document.querySelector('[data-group="packages"] [data-suggestion] a')`);
  assert.equal(await evaluate(`document.querySelector('[data-group="options"] [data-results] a').getAttribute('href').startsWith('/reference/options/#')`), true);
  assert.equal(await evaluate(`document.querySelector('[data-group="packages"] [data-results] a').href.startsWith('https://search.nixos.org/')`), true);
  assert.equal(await evaluate(`getComputedStyle(document.querySelector('.pagefind-ui__drawer')).overflowY`), 'visible');
  assert.equal(await evaluate(`(() => {
    const results = document.querySelector('.pagefind-ui__results-area').getBoundingClientRect();
    const drawer = document.querySelector('.pagefind-ui__drawer').getBoundingClientRect();
    const more = document.querySelector('.search-more-docs').getBoundingClientRect();
    return Math.abs(results.width - drawer.width) < 2 && more.top >= results.bottom;
  })()`), true, 'Docs fill the pane and Show more follows all docs results');
  assert.equal(await evaluate(`document.querySelector('[data-group="packages"] button[aria-label^="Copy configuration for"]') !== null`), true);
  assert.equal(await evaluate(`(() => {
    const docs = document.querySelector('.pagefind-ui__drawer .pagefind-ui__result-link');
    return ['options', 'packages'].every(kind => {
      const link = document.querySelector('[data-group="' + kind + '"] [data-results] .pagefind-ui__result-link');
      const title = link.closest('.pagefind-ui__result-title');
      return link.nextElementSibling?.classList.contains('search-result-description')
        && ['fontSize', 'fontWeight', 'fontFamily', 'color'].every(key => getComputedStyle(link)[key] === getComputedStyle(docs)[key])
        && getComputedStyle(title, '::before').maskImage === getComputedStyle(docs.closest('.pagefind-ui__result-title'), '::before').maskImage;
    });
  })()`), true, 'Options and packages share docs title typography and result icons');
  await evaluate(`document.querySelector('.search-more-docs').click()`);
  assert.equal(await evaluate(`document.querySelector('.pagefind-ui__drawer').classList.contains('search-docs-expanded')`), true);
  console.log('One query returns docs, options, packages, and both resolved suggestions without selecting a source.');

  await evaluate(`document.querySelector('.pagefind-ui__search-clear').click()`);
  await until(`document.querySelector('#devenv-search-results').hidden`);
  assert.equal(await evaluate(`document.querySelectorAll('[data-results] li').length`), 0);
  await evaluate(`
    window.finishedOldQueries = 0;
    window.fetch = async url => {
      const params = new URL(url, location.href).searchParams;
      const q = params.get('q');
      const tool = params.get('tool');
      if (!tool) return originalFetch(url);
      if (q === 'oldquery') {
        await new Promise(resolve => setTimeout(resolve, 1000));
        window.finishedOldQueries++;
      }
      if (tool.startsWith('resolve')) return new Response('unavailable', { status: 502 });
      return Response.json({ data: [{ name: tool === 'search_options' ? 'services.' + q : 'pkgs.' + q, description: '<img src=x onerror=alert(1)>' }] });
    };
    setSearchQuery('oldquery');
  `);
  await until(`window.catalogCalls.length >= 4 && document.querySelector('[data-group="options"] [data-status]').textContent === 'Searching…'`);
  // Allow the debounced old requests to start before replacing the query.
  await new Promise(resolve => setTimeout(resolve, 400));
  await evaluate(`setSearchQuery('newquery')`);
  await until(`document.querySelector('[data-group="options"] [data-results] a')?.textContent === 'services.newquery' && document.querySelector('[data-group="packages"] [data-results] a')?.textContent === 'pkgs.newquery' && window.finishedOldQueries === 4`);
  assert.equal(await evaluate(`document.querySelector('[data-results] img')`), null);
  assert.equal(await evaluate(`document.querySelector('[data-group="packages"] [data-suggestion]').textContent`), '');
  console.log('Stale replies cannot replace either group; resolve failures retain results; remote text stays escaped.');
  await cdp('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: true });
  assert.equal(await evaluate(`document.querySelector('dialog').getBoundingClientRect().width <= 390`), true);
  console.log('Mobile modal fits the viewport.');
} finally {
  socket.close();
  await fetch(`${cdpUrl}/json/close/${target.id}`);
}
