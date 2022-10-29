{ pkgs }: pkgs.writeScriptBin "devenv" ''
#!/usr/bin/env bash

NIX_FLAGS="--extra-experimental-features nix-command --extra-experimental-features flakes"

# current hack to test if we have resolved all Nix annoyances
export FLAKE_FILE=.devenv.flake.nix
export FLAKE_LOCK=devenv.lock

# TODO: get the dev version of NIX
PATH=~/dev/nix/outputs/out/bin:$PATH

function assemble {
  if [[ ! -f devenv.nix ]]; then
    echo "devenv.nix does not exist. Maybe you want to run first $ devenv init"
  fi
  if [[ ! -f devenv.yml ]]; then
    echo "devenv.yml does not exist. Maybe you want to run first $ devenv init"
  fi

  mkdir -p .devenv
  # TODO: validate dev.yml using jsonschema
  cat devenv.yml | ${pkgs.yaml2json}/bin/yaml2json > .devenv/devenv.json
  cp -f ${import ./flake.nix { inherit pkgs; }} $FLAKE_FILE
  chmod +w $FLAKE_FILE
}

mkdir -p $HOME/.devenv
GC_DIR=$HOME/.devenv/$(pwd | sha256sum | head -c 20)

command=$1

case $command in
  up)
    assemble
    procfile=$(nix $NIX_FLAGS build --print-out-paths --impure '.#procfile')
    # TODO: error out if there are no processes set.
    echo Starting processes ...
    # TODO: --env
    ${pkgs.honcho}/bin/honcho start -f $procfile
    ;;
  shell)
    assemble
    nix $NIX_FLAGS develop --impure
    ;;
  init)
    # TODO: allow templates and list them
    echo "" > .envrc
    echo "" > devenv.nix 
    echo "" > devenv.yaml
    ;;
  update)
    assemble
    nix $NIX_FLAGS flake update
    ;;
  ci)
    assemble
    nix $NIX_FLAGS build '.#build' --impure
    ;;
  gc)
    # TODO: check if any of these paths are unreachable and delete them
    nix-store --delete $(ls $GC_DIR)
    exit 1
    ;;
  *)
    echo "Usage: $0 {shell|init|up|gc|update|ci}"
    echo
    echo "Commands:"
    echo 
    echo "init: "
    echo "shell: "
    echo "update: "
    echo "up: "
    echo "gc: "
    echo "ci: "
    echo
    exit 1
esac
''

# TODO: GC: link to $GC_DIR/latest and also $GC_DIR/timestamp