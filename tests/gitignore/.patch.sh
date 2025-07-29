echo "{ env.LOCAL = \"1\";}" > devenv.local.nix
echo "ENV=1" > .env
cat > devenv.local.yaml << EOF
inputs:
  flake-utils:
    url: github:numtide/flake-utils
EOF
