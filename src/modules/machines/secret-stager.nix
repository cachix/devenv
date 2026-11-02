{ lib, pkgs, systemd, secrets, destinations ? { } }:
let
  names = lib.attrNames secrets;
  store = if destinations == { } then "/var/lib/devenv-machines/secrets" else "/var/lib/devenv-machines/bootstrap-secrets";
in
pkgs.writeShellScriptBin "devenv-machines-secrets" ''
  set -euo pipefail
  umask 077
  export PATH=${lib.makeBinPath [ pkgs.coreutils systemd ]}

  names=(${lib.escapeShellArgs names})
  references=(${lib.escapeShellArgs (map (name: secrets.${name}) names)})

  fail() { echo "$1" >&2; exit 1; }
  [ "$#" -ge 3 ] || fail "usage: devenv-machines-secrets ROOT ID values|resolve|commit"
  root=''${1%/}; id=$2; command=$3; shift 3
  ${lib.optionalString (destinations != { }) ''[ -n "$root" ]''}
  case "$id" in ""|*[!A-Za-z0-9-]*) fail "invalid secret set ID" ;; esac
  [ "''${#id}" -le 100 ] || fail "invalid secret set ID"
  store="$root${store}"

  if [ -n "$root" ]; then
    mkdir -p "$root/var/lib/systemd"
    export SYSTEMD_CREDENTIAL_SECRET="$root/var/lib/systemd/credential.secret"
  fi

  if [ "$command" = commit ]; then
    [ "$#" -eq 0 ] || fail "commit takes no arguments"
    [ -d "$store/$id" ] || fail "secret set $id has not been staged"
    ${lib.optionalString (destinations != { }) ''
      resolved_root=$(realpath -e -- "$root")
      placing=
      trap 'if [ -n "$placing" ]; then rm -f -- "$placing"; fi' EXIT HUP INT TERM
      place() {
        dest="$root$2"
        dir=$(dirname -- "$dest")
        case "$(realpath -m -- "$dir")/" in "$resolved_root"/*) ;; *) fail "secret $1 would be written outside $root" ;; esac
        (umask 022; mkdir -p -- "$dir")
        placing=$(mktemp -- "$dir/.devenv-secret.XXXXXX")
        systemd-creds decrypt --name="$1" "$store/$id/$1.cred" "$placing"
        chroot "$root" ${pkgs.coreutils}/bin/chown -- "$3:$4" "''${placing#"$root"}"
        chmod -- "$5" "$placing"
        sync -f "$placing"
        mv -T -- "$placing" "$dest"
        placing=
      }
      ${lib.concatMapStringsSep "\n" (name:
        let secret = destinations.${name};
        in "place ${lib.escapeShellArgs [ name secret.path secret.owner secret.group secret.mode ]}"
      ) (lib.attrNames destinations)}
      rm -rf -- "$store/$id"
      exit 0
    ''}
    ln -sfnT "$id" "$store/.current-$id"
    mv -T "$store/.current-$id" "$store/current"
    sync -f "$store"
    exit 0
  fi
  if systemd-creds has-tpm2 --quiet; then key=host+tpm2; else key=host; fi

  mkdir -p "$store"
  [ ! -e "$store/$id" ] || fail "secret set $id already exists"
  staging="$store/.staging-$id"
  work=
  cleanup() {
    rm -rf -- "$staging"
    if [ -n "$work" ]; then rm -rf -- "$work"; fi
  }
  trap cleanup EXIT HUP INT TERM
  mkdir "$staging"

  seal() {
    systemd-creds encrypt --with-key="$key" --name="$1" "$2" "$staging/$1.cred"
    sealed=$(systemd-creds decrypt --name="$1" "$staging/$1.cred" - | wc -c)
    [ "$sealed" = "$3" ] || fail "secret $1 was truncated"
  }

  read_length() {
    IFS= read -r length || fail "missing payload length"
    case "$length" in ""|*[!0-9]*) fail "invalid payload length" ;; esac
  }

  case "$command" in
    values)
      [ "$#" -eq 0 ] || fail "values takes no arguments"
      for name in "''${names[@]}"; do
        IFS= read -r received || fail "missing secret $name"
        [ "$received" = "$name" ] || fail "expected secret $name"
        read_length
        head -c "$length" | seal "$name" - "$length"
      done
      ;;
    resolve)
      [ "$#" -ge 2 ] || fail "resolve requires a resolver and a reason"
      resolver=$1; reason=$2; shift 2
      case "$resolver" in /nix/store/*) ;; *) fail "resolver must be a store path" ;; esac
      selectors=()
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --profile|--provider) [ "$#" -ge 2 ] || fail "$1 requires a value"; selectors+=("$1" "$2"); shift 2 ;;
          *) fail "unsupported resolver argument $1" ;;
        esac
      done
      work=$(mktemp -d /run/devenv-secretspec.XXXXXX)
      read_length
      head -c "$length" > "$work/secretspec.toml"
      [ "$(stat -c %s -- "$work/secretspec.toml")" = "$length" ] || fail "truncated SecretSpec manifest"
      for index in "''${!names[@]}"; do
        name=''${names[$index]}
        resolved=$(TMPDIR="$work" "$resolver" --file "$work/secretspec.toml" --reason "$reason" \
          get "''${references[$index]}" "''${selectors[@]}")
        resolved=$(realpath -e -- "$resolved")
        case "$resolved" in "$work"/*) ;; *) fail "SecretSpec returned a file outside its private directory" ;; esac
        [ -f "$resolved" ] || fail "SecretSpec did not return a regular file"
        seal "$name" "$resolved" "$(stat -c %s -- "$resolved")"
        rm -f -- "$resolved"
      done
      ;;
    *) fail "unknown command $command" ;;
  esac

  sync -f "$staging"
  mv -T "$staging" "$store/$id"
  sync -f "$store"
''
