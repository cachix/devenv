/** @type {import('tailwindcss').Config} */
module.exports = {
  important: '.tailwind',
  content: [
    './docs/**/*.{js,html}'
  ],
  darkMode: [
    'selector',
    // Targets either the explicit dark mode, or the implicit dark mode.
    // i.e. the toggle is set to automatic so the applied theme is "slate" which is the dark theme.
    ':is([data-md-color-media="(prefers-color-scheme: dark)"], [data-md-color-media="(prefers-color-scheme)"][data-md-color-scheme="slate"])'
  ],
  theme: {
    extend: {
      colors: {
        'dark-accent-fg': '#6da2f3',
        'dark-default-bg': '#0b1016ff',
        'dark-default-fg--light': '#DEDEDE',
        'dark-link': '#9cb8e2',
        'dark-primary-fg': '#0f151dff',
        'devenv-blue': '#425C82',
      }
    },
  },
  plugins: [
    require('tailwindcss-base-font-size')({
      // mkdocs uses 20px as the base font size.
      // Rescale tailwind to match this.
      baseFontSize: 20,
    }),
  ],
}

