#!/usr/bin/env bash
# Run the same suite on real volumes. CI uses disposable hosted runners;
# locally, Linux requires sudo/mount and macOS requires hdiutil.
set -euo pipefail

work=$(mktemp -d /tmp/devenv-fs.XXXXXX)
mountpoint="$work/volume"
mkdir "$mountpoint" "$work/source"
mounted=false
cleanup() {
  if "$mounted"; then
    case "$(uname -s)" in
      Linux) sudo umount "$mountpoint" ;;
      Darwin) hdiutil detach "$mountpoint" ;;
    esac
  fi
  rm -rf "$work"
}
trap cleanup EXIT

case "$(uname -s)" in
  Linux) filesystems=(ext4 btrfs xfs tmpfs) ;;
  Darwin) filesystems=(APFS "Case-sensitive APFS" HFS+) ;;
  *) echo 'This suite supports Linux and macOS' >&2; exit 1 ;;
esac

# Build once, outside the test volumes. No benchmark thresholds in CI.
cargo nextest run -p devenv-tasks --features test-all --no-run
for filesystem in "${filesystems[@]}"; do
  echo "Testing files-reconcile on $filesystem"
  case "$(uname -s)" in
    Linux)
      if [ "$filesystem" = tmpfs ]; then
        sudo mount -t tmpfs -o size=2G tmpfs "$mountpoint"
      else
        truncate -s 2G "$work/disk.img"
        case "$filesystem" in
          ext4) mkfs.ext4 -F "$work/disk.img" ;;
          *) "mkfs.$filesystem" -f "$work/disk.img" ;;
        esac
        sudo mount -o loop "$work/disk.img" "$mountpoint"
      fi
      mounted=true
      sudo chown "$(id -u):$(id -g)" "$mountpoint"
      ;;
    Darwin)
      hdiutil create -size 2g -fs "$filesystem" -volname devenv-files-test "$work/disk.dmg"
      hdiutil attach -nobrowse -mountpoint "$mountpoint" "$work/disk.dmg"
      mounted=true
      ;;
  esac
  mkdir "$mountpoint/tmp"
  TMPDIR="$mountpoint/tmp" DEVENV_FILES_SOURCE_DIR="$work/source" \
    cargo nextest run -j 4 -p devenv-tasks --features test-all \
      -E 'test(builtins::files) | binary(files_builtin_integration) | binary(files_builtin_crash_recovery)'
  case "$(uname -s)" in
    Linux) sudo umount "$mountpoint"; rm -f "$work/disk.img" ;;
    Darwin) hdiutil detach "$mountpoint"; rm "$work/disk.dmg" ;;
  esac
  mounted=false
done
