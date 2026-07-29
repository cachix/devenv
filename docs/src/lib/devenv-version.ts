// Latest devenv release tag, fetched once per build/dev-server process.
// Returns null on network failure so callers can hide the UI gracefully.
let cached: Promise<string | null> | undefined;

export function getDevenvVersion(): Promise<string | null> {
  if (!cached) {
    cached = fetch('https://api.github.com/repos/cachix/devenv/releases/latest', {
      headers: { Accept: 'application/vnd.github+json' },
    })
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => (d && typeof d.tag_name === 'string' ? d.tag_name : null))
      .catch(() => null);
  }
  return cached;
}
