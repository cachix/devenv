{ pkgs }:
let
  examples = ../examples;
in
pkgs.writeScriptBin "devenv" ''
  #!/usr/bin/env bash

  NIX_FLAGS="--show-trace --extra-experimental-features nix-command --extra-experimental-features flakes"

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

    export DEVENV_DIR=$(pwd)/.devenv
    export DEVENV_GC=$DEVENV_DIR/gc
    mkdir -p $DEVENV_GC
    # TODO: validate dev.yml using jsonschema
    cat devenv.yml | ${pkgs.yaml2json}/bin/yaml2json > $DEVENV_DIR/devenv.json
    cp -f ${import ./flake.nix { inherit pkgs; }} $FLAKE_FILE
    chmod +w $FLAKE_FILE
  }

  GC_ROOT=$HOME/.devenv/gc
  mkdir -p $GC_ROOT
  GC_DIR=$GC_ROOT/$(date +%s)

  function add_gc {
    name=$1
    storePath=$2

    nix-store --add-root $DEVENV_GC/$name -r $storePath >/dev/null
    ln -sf $storePath $GC_DIR-$name
  }

  command=$1

  case $command in
    up)
      assemble
      procfile=$(nix $NIX_FLAGS build --print-out-paths --impure '.#procfile')
      procfileenv=$(nix $NIX_FLAGS build --print-out-paths --impure '.#procfileEnv')
      add_gc procfile $procfile
      add_gc procfileenv $procfileenv
      if [ "$(cat $procfile)" = "" ]; then
        echo "No 'processes' option defined."  
        exit 1
      else
        echo Starting processes ...
        ${pkgs.honcho}/bin/honcho start -f $procfile --env $procfileenv
      fi
      ;;
    shell)
      assemble
      env=$(nix $NIX_FLAGS print-dev-env --impure --profile $DEVENV_GC/shell)
      nix-env -p $DEVENV_GC/shell --delete-generations old 2>/dev/null
      ln -sf $(readlink -f $DEVENV_GC/shell) $GC_DIR-shell 
      nix $NIX_FLAGS develop $DEVENV_GC/shell
      ;;
    init)
      # TODO: allow selecting which example and list them
      example=simple
      echo "Creating .envrc"
      cat ${examples}/$example/.envrc > .envrc
      echo "Creating .devenv.nix"
      cat ${examples}/$example/.envrc > devenv.nix 
      echo "Creating .devenv.yaml"
      cat ${examples}/$example/.envrc > devenv.yaml
      echo ".devenv*" >> .gitignore
      echo "Done."
      ;;
    update)
      assemble
      nix $NIX_FLAGS flake update
      ;;
    ci)
      assemble
      ci=$(nix $NIX_FLAGS build --print-out-paths '.#ci' --impure)
      add_gc ci $ci
      ;;
    gc)
      SECONDS=0

      for link in $(${pkgs.findutils}/bin/find $GC_ROOT -type l); do
        if [ ! -f $link ]; then
          unlink $link
        fi
      done

      echo "Counting old devenvs ..."
      echo
      candidates=$(${pkgs.findutils}/bin/find $GC_ROOT -type l)

      before=$(nix $NIX_FLAGS path-info $candidates -S --json | ${pkgs.jq}/bin/jq '[.[].closureSize | tonumber] | add')
      paths=$(nix-store -qR $candidates)

      echo "Found $(echo $paths | wc -w) store paths of sum size $(( $before / 1024 / 1024 )) MB."
      echo
      echo "Garbage collecting ..."
      echo
      echo "Note: If you'd like this command to run much faster, leave a thumbs up at https://github.com/NixOS/nix/issues/7239"

      echo $paths  | tr ' ' '\n' | ${pkgs.parallel}/bin/parallel -j8 nix $NIX_FLAGS store delete >/dev/null 2>/dev/null
  
      # after GC delete links again
      for link in $(${pkgs.findutils}/bin/find $GC_ROOT -type l); do
        if [ ! -f $link ]; then
          unlink $link
        fi
      done

      after=$(nix $NIX_FLAGS path-info $(${pkgs.findutils}/bin/find $GC_ROOT -type l) -S --json | ${pkgs.jq}/bin/jq '[.[].closureSize | tonumber] | add')
      echo
      echo "Done. Saved $((($before - $after) / 1024 / 1024 )) MB in $SECONDS seconds."
      ;;
    *)
      echo "https://devenv.sh (version 0.1): Fast, Declarative, Reproducible, and Composable Developer Environments"
      echo
      echo "Usage: devenv command [options] [arguments]"
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
