#!/usr/bin/env bash
set -euo pipefail

mkdir -p remote/profile/tool remote/profile/nested/tool

cat > remote/profile/devenv.yaml <<'EOF'
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/this-must-be-overridden-by-the-root
  remote-tool:
    url: path:./tool
    flake: false
imports:
  - ./nested
EOF

cat > remote/profile/devenv.nix <<'EOF'
{ inputs, ... }:
{
  env.REMOTE_MODULE = builtins.replaceStrings [ "\n" ] [ "" ]
    (builtins.readFile (inputs.remote-tool + /marker));
}
EOF

cat > remote/profile/tool/marker <<'EOF'
remote-tool
EOF

cat > remote/profile/nested/devenv.yaml <<'EOF'
inputs:
  nested-tool:
    url: path:./tool
    flake: false
EOF

cat > remote/profile/nested/devenv.nix <<'EOF'
{ inputs, ... }:
{
  env.REMOTE_NESTED_MODULE = builtins.replaceStrings [ "\n" ] [ "" ]
    (builtins.readFile (inputs.nested-tool + /marker));
}
EOF

cat > remote/profile/nested/tool/marker <<'EOF'
nested-tool
EOF

git -C remote init -q --initial-branch=main
git -C remote add .
git -C remote \
  -c user.email=test@devenv \
  -c user.name=devenv-test \
  -c commit.gpgsign=false \
  commit -q -m 'remote devenv config'

remote_url="git+file://$(pwd)/remote?dir=profile"
sed "s|@REMOTE@|$remote_url|" devenv.yaml > devenv.yaml.new
mv devenv.yaml.new devenv.yaml
