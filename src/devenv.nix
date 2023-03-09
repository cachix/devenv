{ pkgs, nix }:
let
  examples = ../examples;
  lib = pkgs.lib;
  version = lib.fileContents ./modules/latest-version;
in
pkgs.writeScriptBin "devenv" ''
  #!/usr/bin/env bash

  # we want subshells to fail the program
  set -e

  NIX_FLAGS="--show-trace --extra-experimental-features nix-command --extra-experimental-features flakes --option warn-dirty false"

  # current hack to test if we have resolved all Nix annoyances
  export FLAKE_FILE=.devenv.flake.nix
  export FLAKE_LOCK=devenv.lock

  CUSTOM_NIX=${nix.packages.${pkgs.system}.nix}

  function assemble {
    if [[ ! -f devenv.nix ]]; then
      echo "File devenv.nix does not exist. To get started, run:"
      echo
      echo "  $ devenv init"
      exit 1
    fi

    export DEVENV_DIR="$(pwd)/.devenv"
    export DEVENV_GC="$DEVENV_DIR/gc"
    mkdir -p "$DEVENV_GC"
    if [[ -f devenv.yaml ]]; then
      ${import ./devenv-yaml.nix { inherit pkgs; }}/bin/devenv-yaml "$DEVENV_DIR"
    else
      [[ -f "$DEVENV_DIR/devenv.json" ]] && rm "$DEVENV_DIR/devenv.json"
      [[ -f "$DEVENV_DIR/flake.json" ]] && rm "$DEVENV_DIR/flake.json"
      [[ -f "$DEVENV_DIR/imports.txt" ]] && rm "$DEVENV_DIR/imports.txt"
    fi
    cp -f ${import ./flake.nix { inherit pkgs version; }} "$FLAKE_FILE"
    chmod +w "$FLAKE_FILE"
  }

  if [[ -z "$XDG_DATA_HOME" ]]; then
    GC_ROOT="$HOME/.devenv/gc"
  else 
    GC_ROOT="$XDG_DATA_HOME/devenv/gc"
  fi

  mkdir -p "$GC_ROOT"
  GC_DIR="$GC_ROOT/$(date +%s)"

  function add_gc {
    name=$1
    storePath=$2

    nix-store --add-root "$DEVENV_GC/$name" -r $storePath >/dev/null
    ln -sf $storePath "$GC_DIR-$name"
  }

  function shell {
    assemble
    echo "Building shell ..." 1>&2
    env=$($CUSTOM_NIX/bin/nix $NIX_FLAGS print-dev-env --impure --profile "$DEVENV_GC/shell")
    $CUSTOM_NIX/bin/nix-env -p "$DEVENV_GC/shell" --delete-generations old 2>/dev/null
    ln -sf $(${pkgs.coreutils}/bin/readlink -f "$DEVENV_GC/shell") "$GC_DIR-shell"
  }

  command=$1
  if [[ ! -z $command ]]; then
    shift
  fi

  case $command in
    up)
      shell
      eval "$env"
      procfilescript=$($CUSTOM_NIX/bin/nix $NIX_FLAGS build --no-link --print-out-paths --impure '.#procfileScript')
      if [ "$(cat $procfilescript|tail -n +2)" = "" ]; then
        echo "No 'processes' option defined: https://devenv.sh/processes/"  
        exit 1
      else
        add_gc procfilescript $procfilescript
        $procfilescript
      fi
      ;;
    assemble)
      assemble
      ;;
    print-dev-env)
      shell
      echo "$env"
      ;;
    shell)
      shell
      if [ $# -eq 0 ]; then
        echo "Entering shell ..." 1>&2
        echo "" 1>&2
        $CUSTOM_NIX/bin/nix $NIX_FLAGS develop "$DEVENV_GC/shell"
      else
        set -e
        $CUSTOM_NIX/bin/nix $NIX_FLAGS develop "$DEVENV_GC/shell" -c "$@"
      fi
      ;;
    container)
      assemble
      help=$(cat << 'EOF'
  Usage: container [options] CONTAINER-NAME

  Options:
    --registry         Registry to copy the container to.
    --copy             Copy the container to the registry.
    --copy-args=<args> Arguments passed to `skopeo copy`.
    --docker-run       Execute `docker run`.
  EOF
      )

      eval "$(${pkgs.docopts}/bin/docopts -A subcommand -h "$help" : "$@")"

      export DEVENV_CONTAINER=1
      container="''${subcommand[CONTAINER-NAME]}"

      # build container
      spec=$($CUSTOM_NIX/bin/nix $NIX_FLAGS build --impure --print-out-paths --no-link ".#devenv.containers.\"$container\".derivation")
      echo $spec
    
      # copy container
      if [[ ''${subcommand[--copy]} != false || ''${subcommand[--docker-run]} != false ]]; then
        copyScript=$($CUSTOM_NIX/bin/nix $NIX_FLAGS build --print-out-paths --no-link --impure ".#devenv.containers.\"$container\".copyScript")

        if [[ ''${subcommand[--docker-run]} == true ]]; then
          registry=docker-daemon:
        else
          registry="''${subcommand[--registry]}"
        fi
        $copyScript $spec $registry ''${subcommand[--copy-args]}
      fi

      # docker run
      if [[ ''${subcommand[--docker-run]} != false ]]; then
        $($CUSTOM_NIX/bin/nix $NIX_FLAGS build --print-out-paths --no-link --impure ".#devenv.containers.\"$container\".dockerRun")
      fi
      ;;
    search)
      name=$1
      shift
      assemble
      options=$($CUSTOM_NIX/bin/nix $NIX_FLAGS build --no-link --print-out-paths '.#optionsJSON' --impure)
      results=$($CUSTOM_NIX/bin/nix $NIX_FLAGS search --json nixpkgs $name)
      results_options=$(cat $options/share/doc/nixos/options.json | ${pkgs.jq}/bin/jq "with_entries(select(.key | contains(\"$name\")))")
      if [ "$results" = "{}" ]; then
        echo "No packages found for '$name'."
      else
        ${pkgs.jq}/bin/jq -r '[to_entries[] | {name: ("pkgs." + (.key | split(".") | del(.[0, 1]) | join("."))) } * (.value | { version, description})] | (.[0] |keys_unsorted | @tsv) , (["----", "-------", "-----------"] | @tsv), (.[]  |map(.) |@tsv)' <<< "$results" | ${pkgs.util-linuxMinimal}/bin/column -ts $'\t'
        echo
      fi
      echo
      if [ "$results_options" = "{}" ]; then
        echo "No options found for '$name'."
      else
        ${pkgs.jq}/bin/jq -r '["option","type","default", "description"], ["------", "----", "-------", "-----------"],(to_entries[] | [.key, .value.type, .value.default, .value.description[0:80]]) | @tsv' <<< "$results_options" | ${pkgs.util-linuxMinimal}/bin/column -ts $'\t'
      fi
      echo
      echo "Found $(${pkgs.jq}/bin/jq 'length' <<< "$results") packages and $(${pkgs.jq}/bin/jq 'length' <<< "$results_options") options for '$name'."
      ;;
    init)
      if [ "$#" -eq "1" ]
      then
        target="$1"
        mkdir -p "$target"
        cd "$target"
      fi

      if [[ -f devenv.nix && -f devenv.yaml && -f .envrc ]]; then
        echo "Aborting since devenv.nix, devenv.yaml and .envrc already exist."
        exit 1
      fi

      # TODO: allow selecting which example and list them
      example=simple

      if [[ ! -f .envrc ]]; then
        echo "Creating .envrc"
        cat ${examples}/$example/.envrc > .envrc
      fi

      if [[ ! -f devenv.nix ]]; then
        echo "Creating devenv.nix"
        cat ${examples}/$example/devenv.nix > devenv.nix 
      fi

      if [[ ! -f devenv.yaml ]]; then
        echo "Creating devenv.yaml"
        cat ${examples}/$example/devenv.yaml > devenv.yaml
      fi

      if [[ ! -f .gitignore ]]; then
        touch .gitignore
      fi

      if ! grep -q "devenv" .gitignore; then
        echo "Appending .devenv* and devenv.local.nix to .gitignore"

        echo "" >> .gitignore
        echo "# Devenv" >> .gitignore
        echo ".devenv*" >> .gitignore
        echo "devenv.local.nix" >> .gitignore
        echo "" >> .gitignore
      fi
      echo "Done."

      if command -v direnv &> /dev/null; then
        echo "direnv is installed. Running direnv allow."
        direnv allow
      fi
      ;;
    info)
      assemble
      $CUSTOM_NIX/bin/nix $NIX_FLAGS flake metadata | grep Inputs -A10000
      echo
      $CUSTOM_NIX/bin/nix $NIX_FLAGS eval --raw '.#info' --impure
      ;;
    update)
      assemble
      $CUSTOM_NIX/bin/nix $NIX_FLAGS flake update
      ;;
    version)
      echo "devenv: ${version}"
      ;;
    ci)
      assemble
      ci=$($CUSTOM_NIX/bin/nix $NIX_FLAGS build --no-link --print-out-paths '.#ci' --impure)
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

      before=$($CUSTOM_NIX/bin/nix $NIX_FLAGS path-info $candidates -r -S --json | ${pkgs.jq}/bin/jq '[.[].closureSize | tonumber] | add')

      echo "Found $(echo $candidates | wc -l) environments of sum size $(( $before / 1024 / 1024 )) MB."
      echo
      echo "Garbage collecting ..."
      echo
      echo "Note: If you'd like this command to run much faster, leave a thumbs up at https://github.com/NixOS/nix/issues/7239"

      $CUSTOM_NIX/bin/nix $NIX_FLAGS store delete --recursive $candidates

      # after GC delete links again
      for link in $(${pkgs.findutils}/bin/find $GC_ROOT -type l); do
        if [ ! -f $link ]; then
          unlink $link
        fi
      done

      echo "Done in $SECONDS seconds."
      ;;
    *)
      echo "https://devenv.sh (version ${version}): Fast, Declarative, Reproducible, and Composable Developer Environments"
      echo
      echo "Usage: devenv command [options] [arguments]"
      echo
      echo "Commands:"
      echo
      echo "init                      Scaffold devenv.yaml, devenv.nix, and .envrc inside the current directory."
      echo "init TARGET               Scaffold devenv.yaml, devenv.nix, and .envrc inside TARGET directory."
      echo "search NAME               Search packages matching NAME in nixpkgs input."
      echo "shell                     Activate the developer environment."
      echo "shell CMD [args]          Run CMD with ARGS in the developer environment. Useful when scripting."
      echo "container [options] NAME  Generate a container for NAME. See devenv container --help and http://devenv.sh/containers"
      echo "info                      Print information about the current developer environment."
      echo "update                    Update devenv.lock from devenv.yaml inputs. See http://devenv.sh/inputs/#locking-and-updating-inputs"
      echo "up                        Starts processes in foreground. See http://devenv.sh/processes"
      echo "gc                        Removes old devenv generations. See http://devenv.sh/garbage-collection"
      echo "ci                        Builds your developer environment and make sure all checks pass."
      echo "version                   Display devenv version"
      echo
      exit 1
  esac
''
