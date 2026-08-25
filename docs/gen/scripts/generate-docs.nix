{
  lib,
  languages,
  services,
}:

''
  cat > ../src/snippets/services-all.md <<EOF
    \`\`\`nix
    ${lib.concatStringsSep "\n  " (
      map (lang: "services.${lang}.enable = true;") (builtins.attrNames services)
    )}
    \`\`\`
  EOF
  cat > ../src/snippets/languages-all.md <<EOF
    \`\`\`nix
    ${lib.concatStringsSep "\n  " (
      map (lang: "languages.${lang}.enable = true;") (builtins.attrNames languages)
    )}
    \`\`\`
  EOF
''
