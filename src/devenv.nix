''
#!/bin/sh

function create {
  # TODO: sanity checks: does devenv.nix exist? does devenv.yml exist?
  rm -rf .devnix
  mkdir .devnix
  # TODO: validate dev.yml
  cat dev.yml | yaml2json > .devnix/dev.json
  mv dev.lock .devnix/flake.lock 
  cd .devnix
  cp ${import ./flake.nix { inherit system; }} .devnix/flake.nix
  git init 
  git add .
  cd ..
  # TODO: copy lock back
}

mkdir -p $HOME/.devenv
GC_DIR=$HOME/.devenv/$(pwd | sha256sum | head -c 20)

command=$1

case $command in
  up)
    create
    # TODO: eval modules, run foreman
    procfile=$(nix build)
    foreman start -f $procfile
    ;;
  shell)
    create
    nix develop -f .devnix
    ;;
  init)
    echo "" > .envrc
    echo "" > dev.nix 
    echo "" > dev.yaml
    ;;
  update)
    create
    nix flake update .direnv
    ;;
  gc)
    # TODO: check if any of these paths are unreachable and delete them
    nix-store --delete $(ls $GC_DIR)
    exit 1
    ;;
  *)
    echo "Usage: $0 {shell|init|up|gc|update}"
    exit 1
esac
''