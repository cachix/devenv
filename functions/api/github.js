const repository = 'cachix/devenv';
const ttl = 3600;

export async function onRequestGet({ env }) {
  let stars = null;
  let latestRelease = null;

  try {
    const headers = {
      Accept: 'application/vnd.github+json',
      'User-Agent': 'devenv-docs',
    };
    if (env.GITHUB_TOKEN) {
      headers.Authorization = `Bearer ${env.GITHUB_TOKEN}`;
    }

    const [repositoryResponse, releaseResponse] = await Promise.all([
      fetch(`https://api.github.com/repos/${repository}`, {
        headers,
        cf: { cacheEverything: true, cacheTtl: ttl },
      }),
      fetch(`https://api.github.com/repos/${repository}/releases/latest`, {
        headers,
        cf: { cacheEverything: true, cacheTtl: ttl },
      }),
    ]);

    if (repositoryResponse.ok) {
      const data = await repositoryResponse.json();
      if (typeof data.stargazers_count === 'number') {
        stars = data.stargazers_count;
      }
    }

    if (releaseResponse.ok) {
      const data = await releaseResponse.json();
      if (typeof data.tag_name === 'string') {
        latestRelease = data.tag_name;
      }
    }
  } catch {
    // Metadata is optional; the navigation remains usable without it.
  }

  const successful = stars !== null || latestRelease !== null;
  return Response.json(
    { stars, latestRelease },
    {
      headers: {
        'Cache-Control': successful ? `public, max-age=${ttl}` : 'no-store',
      },
    },
  );
}
