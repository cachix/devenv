import { createMarkdownMiddleware } from '../docs/site-kit-cloudflare.js';

export const onRequest = [createMarkdownMiddleware()];
