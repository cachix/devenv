#!/usr/bin/env bash
set -eEuo pipefail

if [ "${DEBUG:-}" == 1 ]; then
  set -x
fi
PROCFILESCRIPT="${PROCFILESCRIPT:-"placeholder"}"
VERSION="${VERSION:-"placeholder"}"
CUSTOM_NIX_BIN="${CUSTOM_NIX_BIN:-"$(dirname "$(command -v nix)")"}"

NIX_FLAGS=(--show-trace --extra-experimental-features 'nix-command flakes')

function nix {
  "$CUSTOM_NIX_BIN/nix" "${NIX_FLAGS[@]}" "${@}"
}

function container {
  # shellcheck disable=SC1090
  source "$(command -v docopts).sh"
  # shellcheck disable=SC2016
  eval "$(docopts -A args -h '
Usage: container [options] <container-name> [--] [<run-args>...]

Options:
  -s <shell>, --shell <shell>               `devenv.shells.<shell>` to use. [default: default]
  -r <name>, --registry <name>              Registry to copy the container to.
                                            Available shortcuts: config, local-docker, local [default: config]
  -i <name>, --image <name>                 Image name:tag to replace ${container.name}:${container.version} with.
  --copy                                    Copy the container to the registry.
  --copy-args <args>                        Arguments passed to `skopeo copy`.
  --docker-run                              Execute `docker run`.
  --podman-run                              Execute `podman run`.
' : "$@")"

  local run_args=()
  eval "$(docopt_get_eval_array args '<run-args>' run_args)"

  local registry="${args['--registry']}"
  local copy_args=()

  local flake_root="${DEVENV_ROOT:-"${DIRENV_ROOT}"}"
  local app_prefix="${flake_root}#devenv-${args['--shell']}-container-${args['<container-name>']}"

  if [[ "${args['--copy']}" != false || "${args['--docker-run']}" != false || "${args['--podman-run']}" != false ]]; then
    if [[ ${args['--docker-run']} == true ]]; then
      registry=local-docker
    elif [[ ${args['--podman-run']} == true ]]; then
      registry=local
    fi
    if [[ -n "${args['--image']}" ]]; then
      copy_args+=(--image "${args['--image']}")
    fi
    # shellcheck disable=SC2086
    nix run --impure "${app_prefix}-copy-to" -- --registry "${registry}" "${copy_args[@]}" ${args['--copy-args']}
  fi

  if [[ "${args['--docker-run']}" != false ]]; then
    nix run --impure "${app_prefix}-docker-run" -- "${run_args[@]}"
  elif [[ "${args['--podman-run']}" != false ]]; then
    nix run --impure "${app_prefix}-podman-run" -- "${run_args[@]}"
  fi
}

command="${1:-}"
if [[ -n "$command" ]]; then
  shift
fi

case "$command" in
up)
  if [ "$(tail -n +2 <<<"$PROCFILESCRIPT")" = "" ]; then
    echo "No 'processes' option defined: https://devenv.sh/processes/"
    exit 1
  else
    exec "$PROCFILESCRIPT" "$@"
  fi
  ;;
container)
  container "$@"
  ;;
version)
  echo "devenv: ${VERSION}"
  ;;
*)
  echo "https://devenv.sh (version ${VERSION}): Fast, Declarative, Reproducible, and Composable Developer Environments"
  echo
  echo "This is a flake integration wrapper that comes with a subset of functionality from the flakeless devenv CLI."
  echo
  echo "Usage: devenv command"
  echo
  echo "Commands:"
  echo
  echo "up              Starts processes in foreground. See http://devenv.sh/processes"
  echo "version         Display devenv version"
  echo
  exit 1
  ;;
esac
