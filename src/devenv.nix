{ pkgs }: pkgs.writeScriptBin "devenv" ''
#!/bin/sh

function assemble {
  if [[ ! -f devenv.nix ]]; then
    echo "devenv.nix does not exist. Maybe you want to run first $ devenv init"
  fi
  if [[ ! -f devenv.yml ]]; then
    echo "devenv.yml does not exist. Maybe you want to run first $ devenv init"
  fi

  rm -rf .devenv
  mkdir .devenv
  # TODO: validate dev.yml
  cat devenv.yml | ${pkgs.yaml2json}/bin/yaml2json > .devenv/devenv.json
  cp devenv.nix .devenv/devenv.nix
  if [[ -f devenv.lock ]]; then
    cp devenv.lock .devenv/flake.lock 
  fi
  cp ${import ./flake.nix { inherit pkgs; }} .devenv/flake.nix
  pushd .devenv >/dev/null
  git init -q
  git add .
  popd  >/dev/null
}

function updateLock {
  if [[ -f devenv.lock ]]; then
    cp .devenv/flake.lock devenv.lock
  fi
}

mkdir -p $HOME/.devenv
GC_DIR=$HOME/.devenv/$(pwd | sha256sum | head -c 20)

command=$1

case $command in
  up)
    assemble
    procfile=$(nix build)
    ${pkgs.foreman}/bin/foreman start -f $procfile
    ;;
  shell)
    assemble
    nix develop .devenv --impure
    updateLock
    ;;
  init)
    # TODO: allow templates and list them
    echo "" > .envrc
    echo "" > devenv.nix 
    echo "" > devenv.yaml
    ;;
  update)
    assemble
    nix flake update .devenv
    updateLock
    ;;
  gc)
    # TODO: check if any of these paths are unreachable and delete them
    nix-store --delete $(ls $GC_DIR)
    exit 1
    ;;
  *)
    echo "Usage: $0 {shell|init|up|gc|update}"
    echo
    echo "Commands:"
    echo 
    echo "init: "
    echo "shell: "
    echo "update: "
    echo "up: "
    echo "gc: "
    echo
    exit 1
esac
''

# TODO: GC: link to $GC_DIR/latest and also $GC_DIR/timestamp