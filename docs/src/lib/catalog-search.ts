type Kind = 'options' | 'packages';
type Entry = { name: string; description?: string; version?: string; default?: unknown };
type Resolution = { match: Entry | string | null; options?: Entry[]; alternatives?: Array<Entry | { group: string }> };

export function optionUrl(name: string) {
  return `/reference/options/#${name.toLowerCase().replace(/[^a-z0-9_-]+/g, '')}`;
}

export function enhanceSearch() {
  const site = document.querySelector<HTMLElement>('site-search');
  const template = document.querySelector<HTMLTemplateElement>('#devenv-search-template');
  const frame = site?.querySelector<HTMLElement>('.dialog-frame');
  if (!site || !template || !frame || site.dataset.catalogReady) return;
  site.dataset.catalogReady = 'true';
  const content = template.content.cloneNode(true) as DocumentFragment;
  const panel = content.querySelector<HTMLElement>('#devenv-search-results')!;
  frame.append(panel);
  // Pagefind is only available in production; keep remote search usable in dev.
  if (import.meta.env.DEV) {
    const input = document.createElement('input');
    input.type = 'search';
    input.className = 'devenv-catalog-dev-input';
    input.placeholder = 'Search docs, options, and packages';
    input.setAttribute('aria-label', input.placeholder);
    frame.prepend(input);
  }
  const decorateDocs = () => {
    const input = site.querySelector<HTMLInputElement>('input');
    if (input) {
      input.placeholder = 'Search docs, options, and packages';
      input.setAttribute('aria-label', input.placeholder);
    }
    const drawer = site.querySelector<HTMLElement>('.pagefind-ui__drawer');
    if (drawer && !drawer.querySelector('[data-docs-heading]')) {
      const heading = document.createElement('h2');
      heading.dataset.docsHeading = '';
      heading.textContent = 'Docs';
      drawer.prepend(heading);
      const moreDocs = document.createElement('button');
      moreDocs.type = 'button';
      moreDocs.className = 'search-more-docs';
      moreDocs.textContent = 'Show more docs';
      moreDocs.addEventListener('click', () => drawer.classList.add('search-docs-expanded'));
      drawer.append(moreDocs);
    }
    return Boolean(input && drawer);
  };
  const docsObserver = new MutationObserver(() => {
    if (decorateDocs()) docsObserver.disconnect();
  });
  if (!decorateDocs()) docsObserver.observe(frame, { childList: true, subtree: true });
  let query = '';
  let controller: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout>;
  let generation = 0;
  const cache = new Map<string, unknown>();

  function addText(parent: HTMLElement, tag: string, text: string) {
    const element = document.createElement(tag);
    element.textContent = text;
    parent.append(element);
    return element;
  }
  function entryCard(parent: HTMLElement, entry: Entry, kind: Kind) {
    const header = document.createElement('div');
    header.className = 'search-result-header';
    const link = document.createElement('a');
    link.textContent = entry.name;
    if (kind === 'options') link.href = optionUrl(entry.name);
    else {
      const attr = entry.name.replace(/^pkgs\./, '');
      link.href = `https://search.nixos.org/packages?${new URLSearchParams({ channel: 'unstable', show: attr, query: attr })}`;
    }
    header.append(link);
    if (entry.version) addText(header, 'small', entry.version);
    parent.append(header);
    if (entry.description) {
      const description = entry.description.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1').replace(/[`*_]/g, '').replace(/\s+/g, ' ').trim();
      addText(parent, 'p', description);
    }
    if (kind === 'packages') {
      const copy = document.createElement('button');
      copy.type = 'button';
      copy.textContent = 'Copy';
      copy.setAttribute('aria-label', `Copy configuration for ${entry.name}`);
      copy.title = 'Copy configuration';
      copy.addEventListener('click', async () => {
        const name = entry.name.startsWith('pkgs.') ? entry.name : `pkgs.${entry.name}`;
        try { await navigator.clipboard.writeText(`packages = [ ${name} ];`); copy.textContent = 'Copied'; }
        catch { copy.textContent = 'Could not copy'; }
      });
      header.append(copy);
    }
  }
  function createGroup(kind: Kind) {
    const group = panel.querySelector<HTMLElement>(`[data-group="${kind}"]`)!;
    const status = group.querySelector<HTMLElement>('[data-status]')!;
    const suggestion = group.querySelector<HTMLElement>('[data-suggestion]')!;
    const list = group.querySelector<HTMLElement>('[data-results]')!;
    const more = group.querySelector<HTMLButtonElement>('[data-more]')!;
    let entries: Entry[] = [];
    let resolution: Resolution | undefined;
    let visible = 3;
    function render() {
      suggestion.replaceChildren();
      const match = resolution?.match;
      let suggestedName: string | undefined;
      if (match) {
        addText(suggestion, 'span', 'Suggested').className = 'search-suggestion-label';
        if (typeof match === 'string') {
          const members = resolution?.options ?? [];
          const target = members.find(entry => entry.name === `${match}.enable`) ?? members[0];
          if (target) {
            const link = document.createElement('a');
            link.href = optionUrl(target.name);
            link.textContent = match;
            suggestion.append(link);
            addText(suggestion, 'p', `${members.length} options in this group`);
          }
        } else { entryCard(suggestion, match, kind); suggestedName = match.name; }
      }
      const seen = new Set(suggestedName ? [suggestedName] : []);
      const alternatives = (resolution?.alternatives ?? []).filter((entry): entry is Entry => 'name' in entry);
      const results = [...alternatives, ...entries].filter(entry => {
        if (seen.has(entry.name)) return false;
        seen.add(entry.name);
        return true;
      });
      list.replaceChildren();
      for (const entry of results.slice(0, visible)) {
        const item = document.createElement('li');
        entryCard(item, entry, kind);
        list.append(item);
      }
      more.hidden = results.length <= visible;
    }
    more.addEventListener('click', () => { visible += 10; render(); });
    return {
      reset(text: string) {
        entries = []; resolution = undefined; visible = 3;
        render();
        status.textContent = text.length > 200 ? 'Enter no more than 200 characters.' : 'Searching…';
      },
      search(text: string, signal: AbortSignal, current: number) {
        const searchTool = kind === 'options' ? 'search_options' : 'search_packages';
        const resolveTool = kind === 'options' ? 'resolve_option' : 'resolve_package';
        void fetchTool(searchTool, text, signal).then(data => {
          if (current !== generation) return;
          entries = data as Entry[];
          status.textContent = entries.length ? `${entries.length} results` : 'No matches';
          render();
        }).catch(() => {
          if (current === generation && !signal.aborted) status.textContent = 'Search is temporarily unavailable. Press Enter to retry.';
        });
        void fetchTool(resolveTool, text, signal).then(data => {
          if (current !== generation) return;
          resolution = data as Resolution;
          render();
        }).catch(() => { /* Ordinary search remains useful when resolution is unavailable. */ });
      },
    };
  }
  const groups = [createGroup('options'), createGroup('packages')];
  async function fetchTool(tool: string, text: string, signal: AbortSignal) {
    const key = `${tool}:${text}`;
    if (cache.has(key)) return cache.get(key);
    const response = await fetch(`/api/search?${new URLSearchParams({ tool, q: text })}`, { signal });
    if (!response.ok) throw new Error('Search unavailable');
    const payload = await response.json();
    if (cache.size >= 40) cache.delete(cache.keys().next().value!);
    cache.set(key, payload.data);
    return payload.data;
  }

  function search(delay = 300) {
    clearTimeout(timer);
    controller?.abort();
    const current = ++generation;
    const text = query.trim();
    site!.querySelector('.pagefind-ui__drawer')?.classList.remove('search-docs-expanded');
    panel.hidden = !text;
    groups.forEach(group => group.reset(text));
    if (!text || text.length > 200) return;
    timer = setTimeout(() => {
      controller = new AbortController();
      for (const group of groups) group.search(text, controller.signal, current);
    }, delay);
  }
  site.addEventListener('input', event => {
    if (!(event.target instanceof HTMLInputElement)) return;
    query = event.target.value;
    search();
  });
  site.addEventListener('submit', event => {
    event.preventDefault(); search(0);
  });
  site.addEventListener('keydown', event => {
    if (event.key === 'Enter' && event.target instanceof HTMLInputElement) {
      event.preventDefault();
      search(0);
    }
  });
  // Pagefind's clear button updates its input without dispatching an input event.
  site.addEventListener('click', event => {
    if ((event.target as HTMLElement).closest('.pagefind-ui__search-clear')) { query = ''; search(0); }
  });
  site.querySelector('dialog')!.addEventListener('close', () => { clearTimeout(timer); controller?.abort(); generation++; });
  new MutationObserver(() => {
    if (site.querySelector('dialog')!.open) search(0);
  }).observe(site.querySelector('dialog')!, { attributes: true, attributeFilter: ['open'] });
}
