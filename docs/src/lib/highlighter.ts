// Build-time Shiki highlighter for the landing page's static snippets.
// Shares the same theme pair as the docs (Expressive Code) and the live
// hero preview, so every code block matches. Cached per build process.
import { createHighlighterCore, type HighlighterCore } from '@shikijs/core';
import { createOnigurumaEngine } from '@shikijs/engine-oniguruma';
import { codeThemes } from './code-themes';
import nix from '@shikijs/langs/nix';
import bash from '@shikijs/langs/bash';
import toml from '@shikijs/langs/toml';
import yaml from '@shikijs/langs/yaml';

let cached: Promise<HighlighterCore> | undefined;

export function getHighlighter(): Promise<HighlighterCore> {
  if (!cached) {
    cached = createHighlighterCore({
      themes: codeThemes,
      langs: [nix, bash, toml, yaml],
      engine: createOnigurumaEngine(import('shiki/wasm')),
    });
  }
  return cached;
}
