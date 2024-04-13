/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    './docs/**/*.{js,html}'
  ],
  theme: {
    extend: {},
  },
  plugins: [
    require('tailwindcss-base-font-size')({
      // mkdocs uses 20px as the base font size.
      // Rescale tailwind to match this.
      baseFontSize: 20,
    }),
  ],
}

