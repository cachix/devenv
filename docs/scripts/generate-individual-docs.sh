set -ex

# WARN: to only be run from the root of the repo!

mkdir -p docs/src/{supported-languages,supported-services,supported-process-managers}

nix build --no-pure-eval --extra-experimental-features 'flakes nix-command' --show-trace --print-out-paths '.#devenv-generate-individual-docs'
cp -r --no-preserve=all result/docs/individual-docs/* docs/src/
