{
  languages.tailwindcss.enable = true;

  enterTest = ''
    command -v tailwindcss
    command -v tailwindcss-language-server

    # Tailwind CSS v4 is the default.
    tailwindcss --help | grep -q "v4\."

    cat > input.css <<'CSS'
    @import "tailwindcss";
    CSS

    cat > index.html <<'HTML'
    <p class="underline text-center">hello</p>
    HTML

    tailwindcss --input input.css --output output.css
    grep -q '\.underline' output.css
    grep -q '\.text-center' output.css
  '';
}
