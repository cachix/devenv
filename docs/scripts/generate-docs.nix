# WARN: to only be run from the root of the repo!

{
  lib,
  languages,
  services,
}:

''
  cat > docs/src/snippets/services-all.md <<EOF
    \`\`\`nix
    ${lib.concatStringsSep "\n  " (
      map (lang: "services.${lang}.enable = true;") (builtins.attrNames services)
    )}
    \`\`\`
  EOF
  cat > docs/src/snippets/languages-all.md <<EOF
    \`\`\`nix
    ${lib.concatStringsSep "\n  " (
      map (lang: "languages.${lang}.enable = true;") (builtins.attrNames languages)
    )}
    \`\`\`
  EOF
''
