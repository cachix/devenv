import { createGitHubMetadataHandler } from '../../docs/site-kit-cloudflare.js';

export const onRequestGet = createGitHubMetadataHandler({
  repository: 'cachix/devenv',
});
