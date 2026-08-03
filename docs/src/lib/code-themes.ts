// Single source of truth for code syntax-highlight themes.
// Consumed by astro.config.mjs (Expressive Code, build-time), the landing-page
// build-time highlighter, and the live hero preview. Change here only.
//
// Custom "devenv" theme. The broad TextMate scopes keep highlighting
// consistent across Nix, shell, TOML, YAML, and the other docs languages.
import type { ThemeRegistrationRaw } from '@shikijs/core';

const tokens = (c: {
  comment: string;
  string: string;
  keyword: string;
  number: string;
  attr: string;
}) => [
  {
    scope: ['comment', 'punctuation.definition.comment'],
    settings: { foreground: c.comment, fontStyle: 'italic' },
  },
  {
    scope: ['string', 'constant.character.escape'],
    settings: { foreground: c.string },
  },
  {
    scope: [
      'constant.language',
      'keyword',
      'storage.type',
      'storage.modifier',
    ],
    settings: { foreground: c.keyword },
  },
  {
    scope: ['constant.numeric'],
    settings: { foreground: c.number },
  },
  {
    scope: [
      'entity.other.attribute-name',
      'entity.name.function',
      'meta.property-name',
      'support.function',
      'support.type.property-name',
      'constant.other.key',
      'variable.function',
      'variable.parameter',
    ],
    settings: { foreground: c.attr },
  },
];

const dark: ThemeRegistrationRaw = {
  name: 'devenv-dark',
  type: 'dark',
  colors: { 'editor.foreground': '#e4e4e7', 'editor.background': '#1a1a1a' },
  settings: tokens({
    comment: '#7c7c85',
    string: '#9ece8a',
    keyword: '#7aa2f7',
    number: '#e0a363',
    attr: '#7dcfff',
  }),
};

const light: ThemeRegistrationRaw = {
  name: 'devenv-light',
  type: 'light',
  colors: { 'editor.foreground': '#1f2933', 'editor.background': '#ffffff' },
  settings: tokens({
    comment: '#8a8f98',
    string: '#3f8f4f',
    keyword: '#3056b5',
    number: '#b5651d',
    attr: '#0e7490',
  }),
};

export const codeThemes = [dark, light];
export const codeThemeNames = { dark: dark.name!, light: light.name! };
