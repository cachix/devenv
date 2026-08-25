# The environment that devenv integration tests run in.
#
# Scripts that run outside the devenv shell — `.patch.sh`, and `.test.sh` under
# `use_shell: false` — would otherwise depend on whatever the host provides.
# This derivation decides instead. It holds the tools those scripts may use and
# the shell helpers they share.
#
# The helpers call their tools by store path, so they hold even where a test
# rewrites `PATH`.
{
  lib,
  buildEnv,
  runCommand,
  writeText,

  bash,
  coreutils,
  curl,
  diffutils,
  findutils,
  gawk,
  git,
  gnugrep,
  gnused,
  jq,
}:
let
  tools = buildEnv {
    name = "devenv-test-tools";
    pathsToLink = [ "/bin" ];
    paths = [
      bash
      coreutils
      curl
      diffutils
      findutils
      gawk
      git
      gnugrep
      gnused
      jq
    ];
  };

  curl' = lib.getExe curl;
  cut = lib.getExe' coreutils "cut";
  sha256sum = lib.getExe' coreutils "sha256sum";
  sleep = lib.getExe' coreutils "sleep";
  timeout = lib.getExe' coreutils "timeout";

  testLib = writeText "devenv-test-lib.sh" ''
    # Shared shell helpers for devenv integration tests. Generated from
    # devenv-run-tests/test-env.nix; DEVENV_TEST_LIB points at this file.
    #
    # Every bound is in seconds. Waiting helpers poll ten times a second, so a
    # command slower than that stretches the wall clock past the bound.
    #
    # Helpers that take a command take the bound first; helpers that take a
    # subject take it last, where it is optional.

    # Run a command under a failure bound: the command's own status, or 124 when
    # the bound killed it.
    #
    # The bound is a failure bound, not a timing assertion. Like `timeout(1)`,
    # this signals only the direct child, and a non-zero status aborts the caller
    # under `set -e` unless the call is guarded.
    run_bounded() {
      local limit=$1
      shift
      ${timeout} --kill-after=2 "$limit" "$@"
    }

    # Retry a command until it succeeds.
    wait_until() {
      local polls=$(($1 * 10))
      shift
      while [ "$polls" -gt 0 ]; do
        if "$@"; then return 0; fi
        polls=$((polls - 1))
        ${sleep} 0.1
      done
      echo "command never succeeded: $*" >&2
      return 1
    }

    # Whether a local HTTP port answers a request.
    #
    # An empty port is nothing to answer, so it reads as not ready. Tests learn
    # a port at runtime and would otherwise guard every call site.
    http_is_ready() {
      [ -n "''${1:-}" ] || return 1
      ${curl'} -sf -o /dev/null --connect-timeout 1 "http://127.0.0.1:$1/" 2>/dev/null
    }

    # Wait for a local HTTP port to start answering. Defaults to 30 seconds.
    wait_for_http_ready() {
      local port=''${1:?no port given}
      if ! wait_until "''${2:-30}" http_is_ready "$port"; then
        echo "port $port never answered" >&2
        return 1
      fi
    }

    # Wait for a local HTTP port to stop answering. Defaults to 15 seconds.
    #
    # An empty port has nothing to wait for.
    wait_for_http_gone() {
      local port=''${1:-}
      [ -n "$port" ] || return 0
      if ! wait_until "''${2:-15}" _http_is_gone "$port"; then
        echo "port $port still bound" >&2
        return 1
      fi
    }

    _http_is_gone() {
      ! http_is_ready "$1"
    }

    # Wait for a path to disappear. Defaults to 10 seconds.
    #
    # Teardown is not synchronous with the client that observed it: a manager
    # closes its socket before it removes its runtime files, so post-conditions
    # on those files must be polled rather than asserted once.
    wait_for_path_gone() {
      local path=$1
      if ! wait_until "''${2:-10}" _path_is_gone "$path"; then
        echo "$path still present" >&2
        return 1
      fi
    }

    _path_is_gone() {
      [ ! -e "$1" ]
    }

    # Wait for a process to exit. Defaults to 30 seconds.
    wait_for_pid_gone() {
      local pid=$1
      if ! wait_until "''${2:-30}" _pid_is_gone "$pid"; then
        echo "pid $pid still running" >&2
        return 1
      fi
    }

    _pid_is_gone() {
      ! kill -0 "$1" 2>/dev/null
    }

    # The runtime directory devenv derives for the project in `$PWD`.
    devenv_runtime_dir() {
      local dotfile hash
      dotfile="$(pwd -P)/.devenv"
      hash="$(printf '%s' "$dotfile" | ${sha256sum} | ${cut} -c1-7)"
      printf '%s/devenv-%s\n' "''${XDG_RUNTIME_DIR:-/tmp}" "$hash"
    }
  '';
in
runCommand "devenv-test-env"
  {
    passthru = { inherit testLib tools; };
    meta.description = "Shell helpers and tools for devenv integration tests";
  }
  ''
    mkdir -p $out/share/devenv-run-tests
    cp ${testLib} $out/share/devenv-run-tests/test-lib.sh
    ln -s ${lib.getBin tools}/bin $out/bin
  ''
