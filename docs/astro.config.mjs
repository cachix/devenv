import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightBlog from 'starlight-blog';
import starlightLlmsTxt from 'starlight-llms-txt';
import tailwindcss from '@tailwindcss/vite';
import sitemap from '@astrojs/sitemap';
import { codeThemes } from './src/lib/code-themes.ts';

const devGitHubApi = {
  name: 'dev-github-api',
  apply: 'serve',
  enforce: 'pre',
  configureServer(server) {
    server.middlewares.use('/api/github', async (_request, response) => {
      let stars = null;
      let latestRelease = null;
      try {
        const headers = { Accept: 'application/vnd.github+json', 'User-Agent': 'devenv-docs' };
        const [repository, release] = await Promise.all([
          fetch('https://api.github.com/repos/cachix/devenv', { headers }),
          fetch('https://api.github.com/repos/cachix/devenv/releases/latest', { headers }),
        ]);
        if (repository.ok) {
          const data = await repository.json();
          if (typeof data.stargazers_count === 'number') stars = data.stargazers_count;
        }
        if (release.ok) {
          const data = await release.json();
          if (typeof data.tag_name === 'string') latestRelease = data.tag_name;
        }
      } catch {
        // The header hides metadata that cannot be loaded.
      }
      response.setHeader('Content-Type', 'application/json');
      response.end(JSON.stringify({ stars, latestRelease }));
    });
  },
};

export default defineConfig({
  site: 'https://devenv.sh',
  vite: {
    plugins: [tailwindcss(), devGitHubApi],
    server: {
      watch: {
        ignored: ['**/.devenv/**'],
      },
      proxy: {
        '/__devenv_api': {
          target: 'https://devenv.new',
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/__devenv_api/, ''),
        },
      },
    },
  },
  integrations: [
    starlight({
      plugins: [
        starlightBlog({
          title: 'Blog',
          navigation: 'none',
          authors: {
            domenkozar: {
              name: 'Domen Kožar',
              picture: 'https://github.com/domenkozar.png',
              url: 'https://github.com/domenkozar',
            },
            sandydoo: {
              name: 'Sander',
              picture: 'https://github.com/sandydoo.png',
              url: 'https://github.com/sandydoo',
            },
          },
        }),
        starlightLlmsTxt({
          description:
            'devenv is a fast, declarative, reproducible, and composable developer environment tool using Nix. It supports 50+ programming languages, services, processes, tasks, containers, tests, and automated tooling.',
        }),
      ],
      title: 'devenv',
      expressiveCode: {
        themes: codeThemes,
      },
      components: {
        Footer: './src/overrides/Footer.astro',
        Header: './src/overrides/Header.astro',
        Hero: './src/overrides/Hero.astro',
        Pagination: './src/overrides/Pagination.astro',
      },
      logo: {
        light: './src/assets/logo.webp',
        dark: './src/assets/logo-dark.webp',
        replacesTitle: true,
      },
      favicon: '/favicon.svg',
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/cachix/devenv' },
        { icon: 'x.com', label: 'X', href: 'https://x.com/devenv_nix' },
        { icon: 'discord', label: 'Discord', href: 'https://discord.gg/naMgvexb6q' },
      ],
      editLink: {
        baseUrl: 'https://github.com/cachix/devenv/edit/main/docs/src/content/docs/',
      },
      head: [
        {
          tag: 'script',
          attrs: {
            src: 'https://cdn.usefathom.com/script.js',
            'data-site': 'ZLDAHRNN',
            defer: true,
          },
        },
      ],
      customCss: ['./src/styles/tailwind.css', './src/styles/landing.css'],
      sidebar: [
        {
          label: 'Getting Started',
          items: [
            { label: 'Quick Start', slug: 'getting-started' },
            { label: 'Auto Activation', slug: 'auto-activation' },
            { label: 'Basics', slug: 'basics' },
          ],
        },
        {
          label: 'Core Features',
          collapsed: true,
          items: [
            { label: 'Packages', slug: 'packages' },
            { label: 'Scripts', slug: 'scripts' },
            { label: 'Files & Variables', slug: 'files-and-variables' },
            { label: 'Creating Files', slug: 'creating-files' },
            { label: 'SecretSpec', slug: 'integrations/secretspec' },
            { label: 'Tasks', slug: 'tasks' },
            { label: 'Git Hooks', slug: 'git-hooks' },
            { label: 'Tests', slug: 'tests' },
            { label: 'Pinning', slug: 'pinning' },
            { label: 'Inputs', slug: 'inputs' },
            { label: 'Overlays', slug: 'overlays' },
            { label: 'Composing Using Imports', slug: 'composing-using-imports' },
            { label: 'Profiles', slug: 'profiles' },
            { label: 'Extending', slug: 'extending' },
            { label: 'Containers', slug: 'containers' },
            { label: 'Outputs', slug: 'outputs' },
            { label: 'Binary Caching', slug: 'binary-caching' },
            { label: 'Garbage Collection', slug: 'garbage-collection' },
          ],
        },
        {
          label: 'Languages',
          collapsed: true,
          items: [{ autogenerate: { directory: 'languages', collapsed: true } }],
        },
        {
          label: 'Services',
          collapsed: true,
          items: [{ autogenerate: { directory: 'services', collapsed: true } }],
        },
        {
          label: 'Processes',
          collapsed: true,
          items: [
            { label: 'Native process manager', slug: 'processes' },
            { label: 'Attaching', link: '/processes/#attaching-to-running-processes' },
            { label: 'Dependencies', link: '/processes/#dependencies' },
            { label: 'Restart policies', link: '/processes/#restart-policies' },
            { label: 'Ready probes', link: '/processes/#ready-probes' },
            { label: 'File watching', link: '/processes/#file-watching' },
            { label: 'Socket activation', link: '/processes/#socket-activation' },
            { label: 'Watchdog', link: '/processes/#watchdog' },
            {
              label: 'Automatic port allocation',
              link: '/processes/#automatic-port-allocation',
            },
            {
              label: 'Alternative process managers',
              collapsed: true,
              items: [
                { label: 'Overview', slug: 'supported-process-managers' },
                {
                  label: 'process-compose',
                  slug: 'supported-process-managers/process-compose',
                },
                { label: 'overmind', slug: 'supported-process-managers/overmind' },
                { label: 'mprocs', slug: 'supported-process-managers/mprocs' },
                { label: 'hivemind', slug: 'supported-process-managers/hivemind' },
                { label: 'honcho', slug: 'supported-process-managers/honcho' },
              ],
            },
          ],
        },
        {
          label: 'Integrations',
          collapsed: true,
          items: [{ autogenerate: { directory: 'integrations', collapsed: true } }],
        },
        {
          label: 'Editor Support',
          collapsed: true,
          items: [{ autogenerate: { directory: 'editor-support', collapsed: true } }],
        },
        {
          label: 'Guides',
          collapsed: true,
          items: [
            { label: 'Ad-hoc Environments', slug: 'ad-hoc-developer-environments' },
            { label: 'Examples', slug: 'examples' },
            { label: 'Cloud', slug: 'cloud' },
            { label: 'Migrating to 2.0', slug: 'guides/migrating-to-20' },
            { label: 'Monorepo', slug: 'guides/monorepo' },
            { label: 'Polyrepo', slug: 'guides/polyrepo' },
            { label: 'Using with Flakes', slug: 'guides/using-with-flakes' },
            { label: 'Using with Flake Parts', slug: 'guides/using-with-flake-parts' },
          ],
        },
        {
          label: 'Recipes',
          collapsed: true,
          items: [{ autogenerate: { directory: 'recipes', collapsed: true } }],
        },
        {
          label: 'Tools',
          collapsed: true,
          items: [
            { label: 'TUI customization', slug: 'tui-customization' },
            { label: 'LSP', slug: 'lsp' },
            { label: 'MCP', slug: 'mcp' },
            { label: 'REPL', slug: 'repl' },
          ],
        },
        {
          label: 'Reference',
          collapsed: true,
          items: [
            { label: 'Options', link: '/reference/options/' },
            { label: 'YAML Options', slug: 'reference/yaml-options' },
            { label: 'Environment Variables', slug: 'reference/environment-variables' },
          ],
        },
        {
          label: 'Community',
          collapsed: true,
          items: [{ autogenerate: { directory: 'community', collapsed: true } }],
        },
      ],
    }),
    sitemap(),
  ],
});
